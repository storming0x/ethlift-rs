use clap::{App, Arg, SubCommand};
use diffy::{create_patch, PatchFormatter};
use directories::UserDirs;
use ethers_core::types::Chain;
use ethers_etherscan::Client;
use ethers_solc::{remappings::Remapping, ProjectPathsConfig};
use foundry_config::load_config;
use std::error::Error;
use std::path::{Path, PathBuf};

type MyResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct UtilsConfig {
    etherscan_token: String,
    contract_address: String,
    chain_id: u64,
    src: String,
    remappings: Vec<Remapping>,
    file_path: String,
    color_output: bool,
}

pub fn get_args() -> MyResult<UtilsConfig> {
    let app_matches = App::new("eth-lift")
        .version("0.1.0")
        .author("@storming0x")
        .about("Ethereum developer utils written in Rust")
        .subcommand(
            SubCommand::with_name("diff")
                .about("Diff of local solidity file vrs etherscan verified source code")
                .arg(
                    Arg::with_name("src")
                        .short("s")
                        .long("source_code_path")
                        .value_name("SRC")
                        .required(true)
                        .help("Source code folder path"),
                )
                .arg(
                    Arg::with_name("etherscan_token")
                        .short("e")
                        .long("etherscan_token")
                        .value_name("ETHERSCAN_TOKEN")
                        .required(true)
                        .help("Etherscan API token"),
                )
                .arg(
                    Arg::with_name("address")
                        .short("a")
                        .long("contract_address")
                        .value_name("CONTRACT_ADDRESS")
                        .required(true)
                        .help("Contract Address"),
                )
                .arg(
                    Arg::with_name("chain")
                        .short("n")
                        .long("chain_id")
                        .value_name("CHAIN_ID")
                        .default_value("1")
                        .help("Network Id for Contract Address"),
                )
                .arg(
                    Arg::with_name("file_path")
                        .short("f")
                        .long("file_path")
                        .value_name("FILE_PATH")
                        .required(true)
                        .help("Smart contract .sol file path"),
                )
                .arg(
                    Arg::with_name("config_file_path")
                        .short("c")
                        .long("config_path")
                        .value_name("CONFIG_PATH")
                        .help("Config file path for your project"),
                ),
        )
        .get_matches();

    match app_matches.subcommand_matches("ethdiff") {
        Some(matches) => {
            let etherscan_token = matches.value_of("etherscan_token").unwrap();
            let source_code_path = matches.value_of("src").unwrap();
            let file_path = matches.value_of("file_path").unwrap();
            let contract_address = matches.value_of("address").unwrap();

            let config_file_path =
                if let Some(tmp_config_file_path) = matches.value_of("config_file_path") {
                    tmp_config_file_path.to_string()
                } else {
                    detect_config_file_path()
                };

            let chain_id = matches
                .value_of("chain")
                .map(parse_positive_int)
                .transpose()
                .map_err(|e| format!("invalid chain id -- {}", e))?;

            let mapped_remappings = extract_remappings(&config_file_path)?;
            Ok(UtilsConfig {
                file_path: file_path.to_string(),
                chain_id: chain_id.unwrap(),
                etherscan_token: etherscan_token.to_string(),
                src: source_code_path.to_string(),
                remappings: mapped_remappings,
                contract_address: contract_address.to_string(),
                color_output: true,
            })
        }
        _ => Err(From::from("unrecognized command")),
    }
}

pub fn run(config: UtilsConfig) -> MyResult<()> {
    let flattened_file = flatten_file(&config.file_path, &config);

    let etherscan_source_code = get_contract_source_code_etherscan(
        &config.contract_address,
        &config.etherscan_token,
        config.chain_id,
    )
    .unwrap();
    // create compare diff from flattened_file
    print_diff(&flattened_file.unwrap(), &etherscan_source_code, &config);
    Ok(())
}

fn extract_remappings(config_file_path: &str) -> MyResult<Vec<Remapping>> {
    let mapped_remappings = if config_file_path.contains("brownie-config") {
        let remappings = extract_brownie_config_remappings_yaml(config_file_path)?;
        convert_brownie_to_forge_remappings(&remappings)?
    } else {
        // foundry path
        let foundry_cfg = load_config();
        foundry_cfg.get_all_remappings()
    };
    Ok(mapped_remappings)
}

fn parse_positive_int(val: &str) -> MyResult<u64> {
    match val.parse() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(From::from(val)),
    }
}

fn detect_config_file_path() -> String {
    let current_dir_path = std::env::current_dir().expect("error detecting config file path");

    if current_dir_path
        .join(Path::new("brownie-config.yml"))
        .exists()
    {
        current_dir_path
            .join(PathBuf::from("brownie-config.yml"))
            .into_os_string()
            .into_string()
            .unwrap()
    } else {
        current_dir_path
            .join(PathBuf::from("foundry.toml"))
            .into_os_string()
            .into_string()
            .unwrap()
    }
}

fn print_diff(flattened_file: &str, etherscan_source_code: &str, config: &UtilsConfig) {
    let patch = create_patch(flattened_file, etherscan_source_code);

    match config.color_output {
        true => {
            let f = PatchFormatter::new().with_color();
            print!("{}", f.fmt_patch(&patch));
        }
        false => print!("{}", patch),
    }
}

fn convert_brownie_to_forge_remappings(remappings: &[String]) -> MyResult<Vec<Remapping>> {
    let mut vec = Vec::new();

    let parsed_remappings: Vec<(String, String, String)> = remappings
        .iter()
        .map(String::as_str)
        .map(parse_brownie_remapping)
        .map(Result::unwrap)
        .collect();

    match UserDirs::new() {
        Some(user_dirs) => {
            let home = user_dirs.home_dir();
            for (import_name, _, lib_path) in parsed_remappings {
                let package_path = home
                    .join(PathBuf::from(r".brownie/packages/"))
                    .join(lib_path)
                    .into_os_string()
                    .into_string()
                    .unwrap();
                vec.push(Remapping {
                    name: import_name,
                    path: package_path,
                });
            }

            Ok(vec)
        }
        None => Err(From::from("home dir not found!")),
    }
}

fn parse_brownie_remapping(brownie_remapping: &str) -> MyResult<(String, String, String)> {
    let mut vec = brownie_remapping.split('=');

    assert!(vec.clone().count() == 2);

    let import_name = vec.next().unwrap();
    let mut lib_path_split = vec.last().unwrap().split('@');

    assert!(lib_path_split.clone().count() == 2);

    let lib_name_split = lib_path_split.clone().next().unwrap().split('/');
    assert!(lib_name_split.clone().count() == 2);
    let lib_name = lib_name_split.last().unwrap();

    let lib_path = format!(
        "{}@{}",
        lib_path_split.next().unwrap(),
        lib_path_split.last().unwrap()
    );

    Ok((import_name.to_string(), lib_name.to_string(), lib_path))
}

// TODO add command for porting configs
// fn create_foundry_config(remappings: Vec<String>) -> MyResult<()> {
//     let mut config = r#"
//     [default]
//     src = 'contracts'
//     out = 'out'
//     libs = ['lib']
//     remappings = [
//     ]
//     "#
//     .parse::<Value>()
//     .unwrap();

//     config["default"]["remappings"] = Value::from(remappings);

//     let toml_string = toml::to_string(&config).expect("Could not encode TOML value");
//     println!("{}", toml_string);

//     fs::write("foundry.toml", toml_string).expect("Could not write foundry.toml file!");

//     Ok(())
// }

fn extract_brownie_config_remappings_yaml(path: &str) -> MyResult<Vec<String>> {
    let f = std::fs::File::open(path)?;
    let data: serde_yaml::Value = serde_yaml::from_reader(f)?;
    let remappings = data["compiler"]["solc"]["remappings"]
        .as_sequence()
        .expect("key remappings not found in file!");

    let result = remappings
        .iter()
        .map(|st| st.as_str().unwrap().to_owned())
        .collect::<Vec<String>>();

    Ok(result)
}

fn flatten_file(target: &str, config: &UtilsConfig) -> MyResult<String> {
    let project_config = create_project_config(config)?;
    let file_path = std::env::current_dir().unwrap().join(target);
    let flattened_file = project_config.flatten(&file_path)?;
    Ok(flattened_file)
}

fn create_project_config(config: &UtilsConfig) -> MyResult<ProjectPathsConfig> {
    match ProjectPathsConfig::builder()
        .sources(&config.src)
        .remappings(config.remappings.clone())
        .root(std::env::current_dir().unwrap())
        .build()
    {
        Ok(config) => Ok(config),
        _ => Err(From::from("error opening config")),
    }
}

#[tokio::main]
async fn get_contract_source_code_etherscan(
    address: &str,
    api_key: &str,
    chain_id: u64,
) -> MyResult<String> {
    let chain = Chain::try_from(chain_id)?;
    let client = Client::new(chain, api_key).unwrap();
    let meta = client
        .contract_source_code(address.parse().unwrap())
        .await
        .unwrap();
    let code = meta.source_code();
    Ok(code)
}

#[test]
fn test_parse_brownie_remapping() {
    let res = parse_brownie_remapping("@yearnvaults=yearn/yearn-vaults@0.4.3");
    assert!(res.is_ok());
    let (import_name, lib_name, lib_path) = res.unwrap();
    assert_eq!(import_name, "@yearnvaults".to_string());
    assert_eq!(lib_name, "yearn-vaults".to_string());
    assert_eq!(lib_path, "yearn/yearn-vaults@0.4.3".to_string());
}
