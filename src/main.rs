mod april;
mod april_version;
mod reconstruct;

use std::fs::File;

use argh::FromArgs;

/// Command-line tool for applying APRIL patches to dpkg packages.
#[derive(FromArgs, Debug)]
struct Args {
    /// path to the dpkg package
    #[argh(positional)]
    package_path: String,
    /// path to the APRIL configuration file
    #[argh(option, short = 'c', long = "config")]
    april_config_path: String,
    /// reconstruction mode (repack the package instead of installing it, default: false)
    #[argh(switch, short = 'r', long = "reconstruct")]
    reconstruction: bool,
}

fn main() {
    let args: Args = argh::from_env();

    let april_file =
        File::open(&args.april_config_path).expect("Failed to open APRIL configuration file");
    let april_data: Vec<april::AprilPackage> =
        serde_json::from_reader(april_file).expect("Failed to parse APRIL configuration file");
    // TODO: version selection not yet implemented
    let actions = april::plan_actions_from_april_data(&april_data[0])
        .expect("Failed to plan actions from APRIL data");
    if args.reconstruction {
        reconstruct::apply_actions_for_reconstruct(args.package_path, &actions)
            .expect("Failed to apply actions for reconstruct");
    } else {
        unimplemented!("Direct installation mode not yet implemented");
    }
}
