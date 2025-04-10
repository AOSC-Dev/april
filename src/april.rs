//! This module contains the parser for APRIL (AOSC Package Reconstruction Information Listing)

use anyhow::{Result, bail};
use deb822_lossless::{Deb822, Paragraph};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap};

const fn default_false() -> bool {
    false
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AprilPackageScriptOverrides {
    prerm: Option<String>,
    postrm: Option<String>,
    preinst: Option<String>,
    postinst: Option<String>,
    triggers: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AprilPackageOverrides {
    name: Option<String>,
    version: Option<String>,
    arch: Option<String>,
    essential: Option<bool>,
    installed_size: Option<u64>,
    section: Option<String>,
    description: Option<String>,
    depends: Option<Vec<String>>,
    recommends: Option<Vec<String>>,
    suggests: Option<Vec<String>>,
    enhances: Option<Vec<String>>,
    pre_depends: Option<Vec<String>>,
    breaks: Option<Vec<String>>,
    conflicts: Option<Vec<String>>,
    replaces: Option<Vec<String>>,
    provides: Option<Vec<String>>,
    scripts: Option<AprilPackageScriptOverrides>,
    conffiles: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AprilFileOperationPhase {
    #[serde(rename = "unpack")]
    Unpack,
    #[serde(rename = "postinst")]
    Postinst,
}

const fn default_unpack() -> AprilFileOperationPhase {
    AprilFileOperationPhase::Unpack
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "arg", rename_all = "kebab-case")]
pub enum AprilFileOperationType {
    Remove,
    Move(String),
    Copy(String),
    Link(String),
    Patch(String),
    BinaryPatch(String),
    Divert(String),
    Track,
    Overwrite(String),
    Add(String),
    Chmod(u16),
    Mkdir,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AprilFileOperation {
    #[serde(default = "default_unpack")]
    phase: AprilFileOperationPhase,
    #[serde(flatten)]
    operation: AprilFileOperationType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AprilPackage {
    schema: String,
    name: String,
    compatible_versions: String,
    #[serde(default = "default_false")]
    total_conversion: bool,
    overrides: AprilPackageOverrides,
    files: Option<HashMap<String, AprilFileOperation>>,
}

#[derive(Debug)]
pub enum AprilActionType {
    Append,
    Replace,
    Remove,
}

/// Planned actions to be taken on the package (contains internal details)
#[derive(Debug)]
pub enum AprilAction {
    /// run pre-configuration scripts (before running any dpkg commands)
    PreconfigPackage,
    /// use `dpkg --unpack` to unpack the package (this includes running pre-installation scripts)
    UnpackPackage,
    /// use `dpkg --extract` to extract the package to root directory (this does NOT include running pre-/post-installation scripts)
    ExtractPackage,
    /// use `dpkg --configure` to configure the package
    ConfigurePackage,
    /// use `dpkg --install` to install the package (= unpack + configure)
    InstallPackage,
    /// patch package field with specified value
    PatchField {
        field: Cow<'static, str>,
        value: String,
        action: AprilActionType,
    },
    /// clear all control fields and scripts in package
    DropControlData,
    /// add deb822 paragraph to dpkg status
    PutControlChunk { data: String },
    /// patch package script with specified value
    PatchScript {
        /// can only be one of `preinst`, `postinst`, `prerm`, `postrm`, `conffiles`, `triggers`
        file: &'static str,
        content: Option<String>,
        action: AprilActionType,
    },
    /// manipulate specified file with specified action
    PatchFile {
        path: String,
        action: AprilFileOperationType,
    },
}

pub fn validate_april_data(data: &AprilPackage) -> Result<()> {
    // validate schema
    if data.schema != "0" {
        bail!("Invalid schema version, expected 0");
    }

    // for total_conversion data, all mandatory fields should be present
    if data.total_conversion {
        if data.overrides.name.is_none()
            || data.overrides.version.is_none()
            || data.overrides.arch.is_none()
            || data.overrides.installed_size.is_none()
            || data.overrides.section.is_none()
            || data.overrides.description.is_none()
            || data.overrides.depends.is_none()
        {
            bail!("Missing mandatory fields in total_conversion package");
        }
    }

    // TODO: validate other fields as well

    Ok(())
}

fn add_fields_patch_action(
    actions: &mut Vec<AprilAction>,
    values: &Option<Vec<String>>,
    name: &'static str,
) {
    if let Some(v) = values {
        if v.is_empty() {
            actions.push(AprilAction::PatchField {
                field: Cow::Borrowed(name),
                value: String::new(),
                action: AprilActionType::Replace,
            });
        } else {
            for f in v {
                if f.is_empty() {
                    continue;
                }
                if let Some((modifier, value)) = f.split_at_checked(1) {
                    match modifier {
                        "+" => actions.push(AprilAction::PatchField {
                            field: Cow::Borrowed(name),
                            value: value.to_string(),
                            action: AprilActionType::Append,
                        }),
                        "-" => actions.push(AprilAction::PatchField {
                            field: Cow::Borrowed(name),
                            value: value.to_string(),
                            action: AprilActionType::Remove,
                        }),
                        _ => actions.push(AprilAction::PatchField {
                            field: Cow::Borrowed(name),
                            value: f.to_string(),
                            action: AprilActionType::Append,
                        }),
                    }
                }
            }
        }
    }
}

fn add_field_patch_action(field: &Option<String>, name: &'static str) -> Option<AprilAction> {
    if let Some(field) = field {
        if field.is_empty() {
            Some(AprilAction::PatchField {
                field: Cow::Borrowed(name),
                value: String::new(),
                action: AprilActionType::Replace,
            })
        } else {
            Some(AprilAction::PatchField {
                field: Cow::Borrowed(name),
                value: field.clone(),
                action: AprilActionType::Replace,
            })
        }
    } else {
        None
    }
}

pub fn plan_actions_from_april_data(data: &AprilPackage) -> Result<Vec<AprilAction>> {
    let mut actions = Vec::with_capacity(10);

    if data.total_conversion {
        // for total_conversion, drop all control fields and scripts
        actions.push(AprilAction::DropControlData);
    }

    // First, collect all the pre-remove/pre-inst script patches, these need to be applied before any other actions
    if let Some(scripts) = &data.overrides.scripts {
        if let Some(preinst) = &scripts.preinst {
            actions.push(if preinst.is_empty() {
                AprilAction::PatchScript {
                    file: "preinst",
                    content: None,
                    action: AprilActionType::Remove,
                }
            } else {
                AprilAction::PatchScript {
                    file: "preinst",
                    content: Some(preinst.clone()),
                    action: AprilActionType::Replace,
                }
            });
        }
        if let Some(prerm) = &scripts.prerm {
            actions.push(if prerm.is_empty() {
                AprilAction::PatchScript {
                    file: "prerm",
                    content: None,
                    action: AprilActionType::Remove,
                }
            } else {
                AprilAction::PatchScript {
                    file: "prerm",
                    content: Some(prerm.clone()),
                    action: AprilActionType::Replace,
                }
            });
        }

        // triggers patching also needs to be applied before any other actions
        if let Some(triggers) = &scripts.triggers {
            actions.push(if triggers.is_empty() {
                AprilAction::PatchScript {
                    file: "triggers",
                    content: None,
                    action: AprilActionType::Remove,
                }
            } else {
                AprilAction::PatchScript {
                    file: "triggers",
                    content: Some(triggers.clone()),
                    action: AprilActionType::Replace,
                }
            });
        }
    }

    // Pre-Depends patching needs to be applied before pre-configure phase
    add_fields_patch_action(&mut actions, &data.overrides.pre_depends, "Pre-Depends");
    if let Some(action) = add_field_patch_action(&data.overrides.arch, "Architecture") {
        actions.push(action);
    }
    if let Some(action) = add_field_patch_action(&data.overrides.name, "Package") {
        actions.push(action);
    }
    if let Some(action) = add_field_patch_action(
        &data.overrides.installed_size.map(|v| v.to_string()),
        "Installed-Size",
    ) {
        actions.push(action);
    }

    // Then, we need to do a preconfigure on the package
    actions.push(AprilAction::PreconfigPackage);

    // confflies patching needs to be applied before extraction phase
    if let Some(confflies) = &data.overrides.conffiles {
        let new_list = confflies.join("\n");
        if new_list.is_empty() {
            actions.push(AprilAction::PatchScript {
                file: "confflies",
                content: None,
                action: AprilActionType::Remove,
            });
        } else {
            actions.push(AprilAction::PatchScript {
                file: "confflies",
                content: Some(new_list),
                action: AprilActionType::Replace,
            });
        }
    }

    // After that, we extra the package to the root directory
    actions.push(AprilAction::ExtractPackage);

    add_fields_patch_action(&mut actions, &data.overrides.depends, "Depends");
    add_fields_patch_action(&mut actions, &data.overrides.recommends, "Recommends");
    add_fields_patch_action(&mut actions, &data.overrides.conflicts, "Conflicts");
    add_fields_patch_action(&mut actions, &data.overrides.suggests, "Suggests");
    add_fields_patch_action(&mut actions, &data.overrides.breaks, "Breaks");
    add_fields_patch_action(&mut actions, &data.overrides.replaces, "Replaces");
    add_fields_patch_action(&mut actions, &data.overrides.provides, "Provides");
    if let Some(action) = add_field_patch_action(&data.overrides.version, "Version") {
        actions.push(action);
    }
    if let Some(action) = add_field_patch_action(&data.overrides.description, "Description") {
        actions.push(action);
    }
    if let Some(action) = add_field_patch_action(&data.overrides.section, "Section") {
        actions.push(action);
    }
    if let Some(action) = add_field_patch_action(
        &data.overrides.essential.map(|v| {
            if v {
                "yes".to_string()
            } else {
                "no".to_string()
            }
        }),
        "Essential",
    ) {
        actions.push(action);
    }

    // If there are files to be patched after the extraction phase (unpack phase), we need to patch them here
    if let Some(files) = &data.files {
        for (path, operation) in files {
            match operation.phase {
                AprilFileOperationPhase::Unpack => {
                    actions.push(AprilAction::PatchFile {
                        path: path.clone(),
                        action: operation.operation.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    // Then we patch the post-installation/post-remove scripts
    if let Some(scripts) = &data.overrides.scripts {
        if let Some(postinst) = &scripts.postinst {
            actions.push(if postinst.is_empty() {
                AprilAction::PatchScript {
                    file: "postinst",
                    content: None,
                    action: AprilActionType::Remove,
                }
            } else {
                AprilAction::PatchScript {
                    file: "postinst",
                    content: Some(postinst.clone()),
                    action: AprilActionType::Replace,
                }
            });
        }
        if let Some(postrm) = &scripts.postrm {
            actions.push(if postrm.is_empty() {
                AprilAction::PatchScript {
                    file: "postrm",
                    content: None,
                    action: AprilActionType::Remove,
                }
            } else {
                AprilAction::PatchScript {
                    file: "postrm",
                    content: Some(postrm.clone()),
                    action: AprilActionType::Replace,
                }
            });
        }
    }

    // After that, we configure the package
    actions.push(AprilAction::ConfigurePackage);

    // If there are files to be patched after the configuration phase (postinst phase), we need to patch them here
    if let Some(files) = &data.files {
        for (path, operation) in files {
            match operation.phase {
                AprilFileOperationPhase::Postinst => {
                    actions.push(AprilAction::PatchFile {
                        path: path.clone(),
                        action: operation.operation.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    // Return the planned actions

    Ok(actions)
}

#[test]
fn test_april_package_parsing_simple() {
    let input = r#"{
        "schema": "0",
        "name": "libfoo",
        "compatible_versions": ">=1.0 && <2.0",
        "total_conversion": false,
        "overrides": {}
}"#;
    let data: AprilPackage = serde_json::from_str(input).unwrap();
    assert_eq!(data.compatible_versions, ">=1.0 && <2.0");
}

#[test]
fn test_april_package_parsing_example_1() {
    let input = include_str!("../examples/sunloginclient.toml");
    let data = toml::from_str(input).unwrap();
    let plan = plan_actions_from_april_data(&data).unwrap();
    dbg!(plan);
}
