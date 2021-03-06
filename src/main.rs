#[macro_use]
extern crate clap;
extern crate blockchain_gateway_lib;

use clap::App;

fn main() {
    let yaml = load_yaml!("cli.yml");
    let mut app = App::from_yaml(yaml);
    let matches = app.clone().get_matches();

    if let Some(_) = matches.subcommand_matches("config") {
        blockchain_gateway_lib::print_config();
    } else if let Some(_) = matches.subcommand_matches("server") {
        blockchain_gateway_lib::start_server();
    } else if let Some(matches) = matches.subcommand_matches("get_btc_blocks") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::get_btc_blocks(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("get_btc_transaction") {
        let hash = matches.value_of("hash").unwrap();
        blockchain_gateway_lib::get_btc_transaction(&hash);
    } else if let Some(matches) = matches.subcommand_matches("get_btc_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::get_btc_transactions(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("publish_btc_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::publish_btc_transactions(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("get_eth_transaction") {
        let hash = matches.value_of("hash").unwrap();
        blockchain_gateway_lib::get_eth_transaction(&hash);
    } else if let Some(matches) = matches.subcommand_matches("get_eth_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::get_eth_transactions(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("publish_eth_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::publish_eth_transactions(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("get_stq_transaction") {
        let hash = matches.value_of("hash").unwrap();
        blockchain_gateway_lib::get_stq_transaction(&hash);
    } else if let Some(matches) = matches.subcommand_matches("get_stq_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::get_stq_transactions(hash, number);
    } else if let Some(matches) = matches.subcommand_matches("publish_stq_transactions") {
        let param = matches.value_of("number").unwrap_or("1");
        let number: u64 = param.parse().unwrap();
        let hash = matches.value_of("hash").map(|x| x.to_string());
        blockchain_gateway_lib::publish_stq_transactions(hash, number);
    } else {
        let _ = app.print_help();
        println!("\n")
    }
}
