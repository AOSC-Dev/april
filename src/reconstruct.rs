use anyhow::{Result, anyhow};
use base64::Engine;
use deb822_lossless::{Deb822, Paragraph};
use sha2::Digest;
use std::{
    borrow::Cow,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::Builder;
use url::Url;

use crate::april::{AprilAction, AprilActionType, AprilFileOperationType};

#[derive(Debug, PartialEq)]
enum AprilResourceType {
    Inline { content: Vec<u8> },
    External { url: String, sha256: String },
}

fn remove_item_from_string_list(list: &str, item: &str) -> String {
    let mut new_list = list.split(',').map(|s| s.trim()).collect::<Vec<&str>>();
    new_list.retain(|&x| {
        // handle two forms of item, like "foo" and "foo (>= version)"
        (x != item) && (!x.starts_with(&format!("{} (", item)))
    });
    new_list.join(", ")
}

fn apply_field_patch(action: &AprilAction, paragraph: &mut Paragraph) {
    match action {
        AprilAction::PatchField {
            field,
            value,
            action,
        } => {
            let field_value = paragraph.get(field).unwrap_or_default();
            match action {
                AprilActionType::Remove => {
                    paragraph.set(field, &remove_item_from_string_list(&field_value, value))
                }
                AprilActionType::Append => {
                    if field_value.is_empty() {
                        paragraph.set(field, value);
                    } else {
                        let new_value = format!("{}, {}", field_value, value);
                        paragraph.set(field, &new_value);
                    }
                }
                AprilActionType::Replace => {
                    if value.is_empty() {
                        paragraph.remove(field);
                    } else {
                        paragraph.set(field, &value);
                    }
                }
            }
        }
        _ => unreachable!(),
    }
}

fn resolve_path<'a, P: AsRef<Path>>(root: P, path: &'a str) -> Result<PathBuf> {
    let root_path = root.as_ref();
    let file_path = root_path.join(path).canonicalize()?;
    if !file_path.starts_with(root_path) {
        return Err(anyhow!("Invalid file path: {}", path));
    }

    Ok(file_path)
}

fn resolve_resource_uri(uri: &str) -> Result<AprilResourceType> {
    let uri_parts = uri.splitn(3, "::").collect::<Vec<&str>>();
    let resource_type;
    let url;
    let mut sha256sum = None;
    match uri_parts.len() {
        2 => {
            resource_type = uri_parts[0];
            url = uri_parts[1];
        }
        3 => {
            resource_type = uri_parts[0];
            url = uri_parts[2];
            let options = uri_parts[1];
            for option in options.split(';') {
                if option.starts_with("sha256=") {
                    sha256sum = Some(option.split('=').last().unwrap());
                }
            }
        }
        _ => {
            return Err(anyhow!("Invalid resource URI: {}", uri));
        }
    }

    if resource_type != "file" {
        // we only support file resources for now
        return Err(anyhow!("Unsupported resource type: {}", resource_type));
    }
    // parse url
    let parsed_url = Url::parse(url)?;

    match parsed_url.scheme() {
        "http" | "https" => {
            let sha256sum = sha256sum
                .ok_or_else(|| anyhow!("Missing or invalid SHA256 sum in resource URI: {}", url))?;

            Ok(AprilResourceType::External {
                url: url.to_string(),
                sha256: sha256sum.to_string(),
            })
        }
        "data" => {
            let data = parsed_url.path();
            let payload_start = data
                .find(',')
                .ok_or_else(|| anyhow!("Invalid data URI: {}", url))?;
            let is_base64 =
                (payload_start > 6) && &data[payload_start - 6..payload_start] == "base64";
            let payload = if is_base64 {
                base64::engine::general_purpose::STANDARD
                    .decode(data[payload_start + 1..].as_bytes())?
            } else {
                percent_encoding::percent_decode(data[payload_start + 1..].as_bytes()).collect()
            };

            Ok(AprilResourceType::Inline { content: payload })
        }
        _ => {
            return Err(anyhow!("Unsupported scheme in resource URI: {}", url));
        }
    }
}

fn fetch_resource_uri(uri: &str) -> Result<Vec<u8>> {
    let resolved_uri = resolve_resource_uri(uri)?;
    match resolved_uri {
        AprilResourceType::External { url, sha256 } => {
            let mut response = ureq::get(&url).call()?;
            if response.status().is_success() {
                let response_content = response.body_mut().read_to_vec()?;
                let mut hasher = sha2::Sha256::new();
                hasher.update(&response_content);
                let calculated_sha256 = hasher.finalize();
                if hex::encode(calculated_sha256) == sha256 {
                    Ok(response_content)
                } else {
                    return Err(anyhow!(
                        "SHA256 sum mismatch for resource: {}, expected {}, got {}",
                        url,
                        sha256,
                        hex::encode(calculated_sha256)
                    ));
                }
            } else {
                return Err(anyhow!(
                    "Failed to fetch resource: {} (HTTP {})",
                    url,
                    response.status()
                ));
            }
        }
        AprilResourceType::Inline { content } => {
            // no need to fetch inline resources
            Ok(content)
        }
    }
}

fn apply_file_operation<P: AsRef<Path>>(
    root: P,
    path: &str,
    action: &AprilFileOperationType,
) -> Result<()> {
    let file_path = resolve_path(&root, path)?;

    match action {
        AprilFileOperationType::Remove => Ok(std::fs::remove_file(&file_path)?),
        AprilFileOperationType::Move(dst) => {
            let dst_path = resolve_path(&root, dst)?;
            std::fs::rename(&file_path, &dst_path)?;
            Ok(())
        }
        AprilFileOperationType::Copy(dst) => {
            let dst_path = resolve_path(&root, dst)?;
            std::fs::copy(&file_path, &dst_path)?;
            Ok(())
        }
        AprilFileOperationType::Link(dst) => {
            let dst_path = resolve_path(&root, dst)?;
            std::os::unix::fs::symlink(&file_path, &dst_path)?;
            Ok(())
        }
        AprilFileOperationType::Patch(url) => {
            let content = fetch_resource_uri(url)?;
            let mut command = Command::new("patch")
                .args(&["-Nt", "-r-"])
                .arg(&file_path)
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            command.stdin.take().unwrap().write_all(&content)?;
            let status = command.wait()?;

            if !status.success() {
                Err(anyhow!("Failed to apply patch: {}", status))
            } else {
                Ok(())
            }
        }
        AprilFileOperationType::BinaryPatch(url) => {
            let content = fetch_resource_uri(url)?;
            let mut command = Command::new("xdelta3")
                .args(&["-d", "-f", "-s"])
                .arg(&file_path)
                .arg("/dev/stdin")
                .arg(&file_path.clone())
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            command.stdin.take().unwrap().write_all(&content)?;
            let status = command.wait()?;

            if !status.success() {
                Err(anyhow!("Failed to apply binary patch: {}", status))
            } else {
                Ok(())
            }
        }
        AprilFileOperationType::Divert(dst) => todo!(),
        AprilFileOperationType::Track => todo!(),
        AprilFileOperationType::Overwrite(url) => {
            let content = fetch_resource_uri(url)?;
            std::fs::write(&file_path, &content)?;
            Ok(())
        }
        AprilFileOperationType::Add(url) => {
            let content = fetch_resource_uri(url)?;
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&file_path)?;
            f.write_all(&content)?;
            Ok(())
        }
        AprilFileOperationType::Chmod(mode) => {
            let result = unsafe {
                libc::chmod(
                    file_path.as_os_str().as_encoded_bytes().as_ptr() as *const libc::c_char,
                    *mode as libc::mode_t,
                )
            };

            if result != 0 {
                let err = std::io::Error::last_os_error();
                Err(err.into())
            } else {
                Ok(())
            }
        }
        AprilFileOperationType::Mkdir => Ok(std::fs::create_dir_all(&file_path)?),
    }
}

fn apply_script_actions<P: AsRef<Path>>(
    root: P,
    file: &str,
    content: &Option<String>,
    action: &AprilActionType,
    installed_name: &Option<String>,
) -> Result<()> {
    let filename = match installed_name {
        Some(installed_name) => {
            let filename = format!("{}.{}", installed_name, file);

            Cow::Owned(filename)
        }
        None => Cow::Borrowed(file),
    };
    let file_path = resolve_path(root.as_ref().join("DEBIAN"), &filename)?;

    match action {
        AprilActionType::Remove => Ok(std::fs::remove_file(&file_path)?),
        AprilActionType::Append => {
            if let Some(content) = content {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)?
                    .write_all(content.as_bytes())?;
            }
            Ok(())
        }
        AprilActionType::Replace => {
            if let Some(content) = content {
                std::fs::write(&file_path, content.as_bytes())?;
            } else {
                return Err(anyhow!("Missing content for replace action"));
            }
            Ok(())
        }
    }
}

pub fn apply_actions_for_reconstruct<P: AsRef<Path>>(
    deb_path: P,
    actions: &[AprilAction],
) -> Result<()> {
    let deb_path = deb_path.as_ref();
    let deb_path_dir = deb_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid package path: {}", deb_path.display()))?;
    let tmp_root = Builder::new().tempdir_in(deb_path_dir)?;
    let status = Command::new("dpkg-deb")
        .arg("-R")
        .arg(deb_path)
        .arg(tmp_root.path())
        .spawn()?
        .wait()?;
    if !status.success() {
        return Err(anyhow!("Failed to extract package: {}", status));
    }

    let control_file_path = tmp_root.path().join("DEBIAN/control");
    let mut control_data = Deb822::from_file(&control_file_path)?;

    for i in actions {
        match i {
            AprilAction::PreconfigPackage
            | AprilAction::UnpackPackage
            | AprilAction::ExtractPackage
            | AprilAction::ConfigurePackage
            | AprilAction::InstallPackage => (),
            AprilAction::PatchField { .. } => {
                for mut paragraph in &mut control_data.paragraphs() {
                    apply_field_patch(&i, &mut paragraph);
                }
            }
            AprilAction::DropControlData => control_data = Deb822::new(),
            AprilAction::PutControlChunk { data } => {
                (control_data, _) = Deb822::from_str_relaxed(data);
            }
            AprilAction::PatchScript {
                file,
                content,
                action,
            } => apply_script_actions(&tmp_root, file, content, action, &None)?,
            AprilAction::PatchFile { path, action } => {
                apply_file_operation(&tmp_root, path, action)?
            }
        }
    }

    std::fs::write(control_file_path, control_data.to_string())?;
    let new_deb_path = deb_path.with_extension(".repacked.deb");
    let status = Command::new("dpkg-deb")
        .arg("-b")
        .arg(tmp_root.path())
        .arg(new_deb_path)
        .spawn()?
        .wait()?;
    if !status.success() {
        return Err(anyhow!("Failed to repack package: {}", status));
    }

    Ok(())
}

#[test]
fn test_remove_item_from_string_list() {
    let input = "foo, bar, baz";
    let item = "bar";
    let expected = "foo, baz";
    assert_eq!(remove_item_from_string_list(input, item), expected);

    let input = "foo, bar (>= 1.2.0), baz";
    let item = "bar";
    let expected = "foo, baz";
    assert_eq!(remove_item_from_string_list(input, item), expected);
}

#[test]
fn test_apply_field_patch() {
    let mut paragraph = Paragraph::new();
    paragraph.set("Depends", "foo (>= 1.2.0), bar");

    let action = AprilAction::PatchField {
        field: std::borrow::Cow::Borrowed("Depends"),
        value: "baz".to_owned(),
        action: AprilActionType::Remove,
    };
    apply_field_patch(&action, &mut paragraph);
    assert_eq!(paragraph.get("Depends").unwrap(), "foo (>= 1.2.0), bar");

    let action = AprilAction::PatchField {
        field: std::borrow::Cow::Borrowed("Depends"),
        value: "baz".to_owned(),
        action: AprilActionType::Append,
    };
    apply_field_patch(&action, &mut paragraph);
    assert_eq!(
        paragraph.get("Depends").unwrap(),
        "foo (>= 1.2.0), bar, baz"
    );

    let action = AprilAction::PatchField {
        field: std::borrow::Cow::Borrowed("Depends"),
        value: "foo".to_owned(),
        action: AprilActionType::Replace,
    };
    apply_field_patch(&action, &mut paragraph);
    assert_eq!(paragraph.get("Depends").unwrap(), "foo");

    let action = AprilAction::PatchField {
        field: std::borrow::Cow::Borrowed("Depends"),
        value: "".to_owned(),
        action: AprilActionType::Replace,
    };
    apply_field_patch(&action, &mut paragraph);
    assert_eq!(paragraph.get("Depends"), None);

    let action = AprilAction::PatchField {
        field: std::borrow::Cow::Borrowed("Depends"),
        value: "baz".to_owned(),
        action: AprilActionType::Append,
    };
    apply_field_patch(&action, &mut paragraph);
    assert_eq!(paragraph.get("Depends").unwrap(), "baz");
}

#[test]
fn test_out_of_bound_file_operation() {
    if let Err(e) = resolve_path("/tmp", "..") {
        assert_eq!(e.to_string(), "Invalid file path: ..");
    } else {
        unreachable!();
    }
}

#[test]
fn test_resolve_resource_uri() {
    let uri = "file::sha256=abc::https://example.com/package.deb".to_string();
    let expected = AprilResourceType::External {
        url: "https://example.com/package.deb".to_string(),
        sha256: "abc".to_string(),
    };
    assert_eq!(resolve_resource_uri(&uri).unwrap(), expected);

    let uri = "file::data:application/octet-stream;base64,SGVsbG8sIHdvcmxkIQ==".to_string();
    let expected = AprilResourceType::Inline {
        content: (&b"Hello, world!"[..]).to_vec(),
    };
    assert_eq!(resolve_resource_uri(&uri).unwrap(), expected);
}
