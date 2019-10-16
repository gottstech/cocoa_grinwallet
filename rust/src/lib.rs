// Copyright 2019 Ivan Sorokin.
// Modifications Copyright 2019 The Gotts Developers
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Libs Wallet External API Definition

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use grin_wallet_api::{Foreign, Owner};
use grin_wallet_config::{self, GrinRelayConfig, WalletConfig};
use grin_wallet_controller::{grinrelay_address, grinrelay_listener};
use grin_wallet_impls::{
    instantiate_wallet, Error, ErrorKind, FileWalletCommAdapter, GrinrelayWalletCommAdapter,
    HTTPNodeClient, HTTPWalletCommAdapter, LMDBBackend, WalletSeed,
};
use grin_wallet_libwallet::api_impl::types::InitTxArgs;
use grin_wallet_libwallet::{NodeClient, SlateVersion, VersionedSlate, WalletInst};
use grin_wallet_util::grin_core::global::ChainTypes;
use grin_wallet_util::grin_keychain::ExtKeychain;
use grin_wallet_util::grin_util::{Mutex, ZeroingString};

/// Default balance minimum confirmation
pub const MINIMUM_CONFIRMATIONS: u64 = 10;

/// Default sending coins selection minimum confirmation
pub const SENDING_MINIMUM_CONFIRMATIONS: u64 = 0;

fn cstr_to_str(s: *const c_char) -> String {
    unsafe { CStr::from_ptr(s).to_string_lossy().into_owned() }
}

#[no_mangle]
pub extern "C" fn cstr_free(s: *mut c_char) {
    unsafe {
        if s.is_null() {
            return;
        }
        // Recover the CString so rust can deallocate it
        CString::from_raw(s)
    };
}

unsafe fn result_to_cstr(res: Result<String, Error>, error: *mut u8) -> *const c_char {
    match res {
        Ok(res) => {
            *error = 0;
            CString::new(res).unwrap().into_raw()
        }
        Err(e) => {
            *error = 1;
            CString::new(serde_json::to_string(&format!("{}", e)).unwrap())
                .unwrap()
                .into_raw()
        }
    }
}

unsafe fn result2_to_cstr(res: Result<(bool, String), Error>, error: *mut u8) -> *const c_char {
    match res {
        Ok((validated, res)) => {
            if validated {
                *error = 0;
            } else {
                *error = 2;
            }
            CString::new(res).unwrap().into_raw()
        }
        Err(e) => {
            *error = 1;
            CString::new(serde_json::to_string(&format!("{}", e)).unwrap())
                .unwrap()
                .into_raw()
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct MobileWalletCfg {
    account: String,
    chain_type: String,
    data_dir: String,
    node_api_addr: String,
    node_api_secret: String,
    password: String,
    minimum_confirmations: u64,
    grinrelay_config: Option<GrinRelayConfig>,
}

impl MobileWalletCfg {
    pub fn from_str(json_cfg: &str) -> Result<Self, Error> {
        serde_json::from_str::<MobileWalletCfg>(json_cfg)
            .map_err(|e| Error::from(ErrorKind::GenericError(e.to_string())))
    }
}

fn new_wallet_config(config: MobileWalletCfg) -> Result<WalletConfig, Error> {
    let chain_type = match config.chain_type.as_str() {
        "mainnet" => ChainTypes::Mainnet,
        "floonet" => ChainTypes::Floonet,
        _ => {
            return Err(Error::from(ErrorKind::GenericError(
                "unsupported chain type".to_owned(),
            )));
        }
    };

    Ok(WalletConfig {
        chain_type: Some(chain_type),
        api_listen_interface: "127.0.0.1".to_string(),
        api_listen_port: 3415,
        owner_api_listen_port: Some(3420),
        api_secret_path: Some(".api_secret".to_string()),
        node_api_secret: Some(config.node_api_secret),
        check_node_api_http_addr: config.node_api_addr,
        owner_api_include_foreign: Some(false),
        data_file_dir: config.data_dir + "/wallet_data",
        no_commit_cache: Some(false),
        tls_certificate_file: None,
        tls_certificate_key: None,
        dark_background_color_scheme: Some(true),
        keybase_notify_ttl: Some(1440),
        grinrelay_config: Some(config.grinrelay_config.clone().unwrap_or_default()),
    })
}

fn select_node_server(check_node_api_http_addr: &str) -> Result<String, Error> {
    // Select nearest node server
    if check_node_api_http_addr
        .starts_with("https://nodes.grin.icu")
    {
        match grin_wallet_config::select_node_server(check_node_api_http_addr) {
            Ok(best) => {
                return Ok(best);
            }
            Err(e) => {
                // error!("select_node_server fail on {}", e);
                return Err(ErrorKind::GenericError(e.to_string()).into());
            }
        }
    }
    Ok(check_node_api_http_addr.to_owned())
}

#[no_mangle]
pub extern "C" fn select_nearest_node(
    check_node_api_http_addr: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = select_node_server(&cstr_to_str(check_node_api_http_addr));
    unsafe { result_to_cstr(res, error) }
}

fn check_password(json_cfg: &str, password: &str) -> Result<String, Error> {
    let wallet_config = new_wallet_config(MobileWalletCfg::from_str(json_cfg)?)?;
    WalletSeed::from_file(&wallet_config.data_file_dir, password).map_err(|e| Error::from(e))?;
    Ok("OK".to_owned())
}

#[no_mangle]
pub extern "C" fn grin_check_password(
    json_cfg: *const c_char,
    password: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = check_password(&cstr_to_str(json_cfg), &cstr_to_str(password));
    unsafe { result_to_cstr(res, error) }
}

fn init_wallet_seed() -> Result<String, Error> {
    WalletSeed::init_new(32).to_mnemonic()
}

#[no_mangle]
pub extern "C" fn grin_init_wallet_seed(error: *mut u8) -> *const c_char {
    let res = init_wallet_seed();
    unsafe { result_to_cstr(res, error) }
}

fn wallet_init(json_cfg: &str, password: &str, is_12_phrases: bool) -> Result<String, Error> {
    let wallet_config = new_wallet_config(MobileWalletCfg::from_str(json_cfg)?)?;
    let node_api_secret = wallet_config.node_api_secret.clone();
    let seed_length = if is_12_phrases { 16 } else { 32 };
    let seed = WalletSeed::init_file(
        &wallet_config.data_file_dir,
        seed_length,
        None,
        password,
        false,
    )?;
    let node_client = HTTPNodeClient::new(&wallet_config.check_node_api_http_addr, node_api_secret);
    let _: LMDBBackend<HTTPNodeClient, ExtKeychain> =
        LMDBBackend::new(wallet_config, password, node_client)?;
    seed.to_mnemonic()
}

#[no_mangle]
pub extern "C" fn grin_wallet_init(
    json_cfg: *const c_char,
    password: *const c_char,
    is_12_phrases: bool,
    error: *mut u8,
) -> *const c_char {
    let res = wallet_init(
        &cstr_to_str(json_cfg),
        &cstr_to_str(password),
        is_12_phrases,
    );
    unsafe { result_to_cstr(res, error) }
}

fn wallet_init_recover(json_cfg: &str, mnemonic: &str) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet_config = new_wallet_config(config.clone())?;
    WalletSeed::recover_from_phrase(
        &wallet_config.data_file_dir,
        mnemonic,
        config.password.as_str(),
    )?;
    let node_api_secret = wallet_config.node_api_secret.clone();
    let node_client = HTTPNodeClient::new(&wallet_config.check_node_api_http_addr, node_api_secret);
    let _: LMDBBackend<HTTPNodeClient, ExtKeychain> =
        LMDBBackend::new(wallet_config, config.password.as_str(), node_client)?;
    Ok("OK".to_owned())
}

#[no_mangle]
pub extern "C" fn grin_wallet_init_recover(
    json_cfg: *const c_char,
    mnemonic: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = wallet_init_recover(&cstr_to_str(json_cfg), &cstr_to_str(mnemonic));
    unsafe { result_to_cstr(res, error) }
}

fn wallet_change_password(
    json_cfg: &str,
    old_password: &str,
    new_password: &str,
) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);

    api.change_password(&Some(ZeroingString::from(old_password)), new_password)
        .map_err(|e| Error::from(e))?;
    Ok("OK".to_owned())
}

#[no_mangle]
pub extern "C" fn grin_wallet_change_password(
    json_cfg: *const c_char,
    old_password: *const c_char,
    new_password: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = wallet_change_password(
        &cstr_to_str(json_cfg),
        &cstr_to_str(old_password),
        &cstr_to_str(new_password),
    );
    unsafe { result_to_cstr(res, error) }
}

fn wallet_restore(json_cfg: &str, start_index: u64, batch_size: u64) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet_config = new_wallet_config(config.clone())?;
    let node_api_secret = wallet_config.node_api_secret.clone();
    let node_client = HTTPNodeClient::new(&wallet_config.check_node_api_http_addr, node_api_secret);
    let wallet = instantiate_wallet(
        wallet_config,
        node_client,
        config.password.as_str(),
        &config.account,
    )?;
    let api = Owner::new(wallet.clone());

    let (highest_index, last_retrieved_index, num_of_found) = api
        .restore_batch(start_index, batch_size)
        .map_err(|e| Error::from(e))?;
    Ok(json!({
        "highestIndex": highest_index,
        "lastRetrievedIndex": last_retrieved_index,
        "numberOfFound": num_of_found,
    })
    .to_string())
}

#[no_mangle]
pub extern "C" fn grin_wallet_restore(
    json_cfg: *const c_char,
    start_index: u64,
    batch_size: u64,
    error: *mut u8,
) -> *const c_char {
    let res = wallet_restore(&cstr_to_str(json_cfg), start_index, batch_size);
    unsafe { result_to_cstr(res, error) }
}

fn wallet_check(
    json_cfg: &str,
    start_index: u64,
    batch_size: u64,
    update_outputs: bool,
) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let (highest_index, last_retrieved_index) = api
        .check_repair_batch(true, start_index, batch_size, update_outputs)
        .map_err(|e| Error::from(e))?;

    Ok(json!({
        "highestIndex": highest_index,
        "lastRetrievedIndex": last_retrieved_index,
    })
    .to_string())
}

#[no_mangle]
pub extern "C" fn grin_wallet_check(
    json_cfg: *const c_char,
    start_index: u64,
    batch_size: u64,
    update_outputs: bool,
    error: *mut u8,
) -> *const c_char {
    let res = wallet_check(
        &cstr_to_str(json_cfg),
        start_index,
        batch_size,
        update_outputs,
    );
    unsafe { result_to_cstr(res, error) }
}

fn get_wallet_mnemonic(json_cfg: &str) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet_config = new_wallet_config(config.clone())?;
    let seed = WalletSeed::from_file(&wallet_config.data_file_dir, config.password.as_str())?;
    seed.to_mnemonic()
}

#[no_mangle]
pub extern "C" fn grin_get_wallet_mnemonic(
    json_cfg: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = get_wallet_mnemonic(&cstr_to_str(json_cfg));
    unsafe { result_to_cstr(res, error) }
}

fn get_wallet_instance(
    config: MobileWalletCfg,
) -> Result<Arc<Mutex<dyn WalletInst<impl NodeClient, ExtKeychain>>>, Error> {
    let wallet_config = new_wallet_config(config.clone())?;
    let node_api_secret = wallet_config.node_api_secret.clone();
    let node_client = HTTPNodeClient::new(&wallet_config.check_node_api_http_addr, node_api_secret);

    instantiate_wallet(
        wallet_config,
        node_client,
        config.password.as_str(),
        config.account.as_str(),
    )
}

fn get_balance(json_cfg: &str) -> Result<(bool, String), Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let (validated, wallet_info) = api.retrieve_summary_info(true, MINIMUM_CONFIRMATIONS)?;
    Ok((validated, serde_json::to_string(&wallet_info).unwrap()))
}

#[no_mangle]
pub extern "C" fn grin_get_balance(json_cfg: *const c_char, error: *mut u8) -> *const c_char {
    let res = get_balance(&cstr_to_str(json_cfg));
    unsafe { result2_to_cstr(res, error) }
}

fn tx_retrieve(json_cfg: &str, tx_slate_id: &str) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let uuid = Uuid::parse_str(tx_slate_id).map_err(|e| ErrorKind::GenericError(e.to_string()))?;
    let txs = api.retrieve_txs(true, None, Some(uuid))?;
    Ok(serde_json::to_string(&txs).unwrap())
}

#[no_mangle]
pub extern "C" fn grin_tx_retrieve(
    json_cfg: *const c_char,
    tx_slate_id: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = tx_retrieve(&cstr_to_str(json_cfg), &cstr_to_str(tx_slate_id));
    unsafe { result_to_cstr(res, error) }
}

fn txs_retrieve(json_cfg: &str) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);

    match api.retrieve_txs(true, None, None) {
        Ok(txs) => Ok(serde_json::to_string(&txs).unwrap()),
        Err(e) => Err(Error::from(e)),
    }
}

#[no_mangle]
pub extern "C" fn grin_txs_retrieve(state_json: *const c_char, error: *mut u8) -> *const c_char {
    let res = txs_retrieve(&cstr_to_str(state_json));
    unsafe { result_to_cstr(res, error) }
}

fn outputs_retrieve(json_cfg: &str, tx_id: Option<u32>) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let outputs = api.retrieve_outputs(true, true, tx_id)?;
    Ok(serde_json::to_string(&outputs).unwrap())
}

#[no_mangle]
pub extern "C" fn grin_output_retrieve(
    json_cfg: *const c_char,
    tx_id: u32,
    error: *mut u8,
) -> *const c_char {
    let res = outputs_retrieve(&cstr_to_str(json_cfg), Some(tx_id));
    unsafe { result_to_cstr(res, error) }
}

#[no_mangle]
pub extern "C" fn grin_outputs_retrieve(json_cfg: *const c_char, error: *mut u8) -> *const c_char {
    let res = outputs_retrieve(&cstr_to_str(json_cfg), None);
    unsafe { result_to_cstr(res, error) }
}

fn init_send_tx(
    json_cfg: &str,
    amount: u64,
    selection_strategy: &str,
    target_slate_version: Option<u16>,
    message: &str,
) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let tx_args = InitTxArgs {
        src_acct_name: None,
        amount,
        minimum_confirmations: SENDING_MINIMUM_CONFIRMATIONS,
        max_outputs: 500,
        num_change_outputs: 1,
        selection_strategy: selection_strategy.to_string(),
        message: Some(message.to_string()),
        target_slate_version,
        estimate_only: None,
        send_args: None,
    };
    let slate = api.init_send_tx(tx_args)?;
    api.tx_lock_outputs(&slate, 0)?;
    Ok(serde_json::to_string(&slate).expect("fail to serialize slate to json string"))
}

#[no_mangle]
pub extern "C" fn grin_init_tx(
    json_cfg: *const c_char,
    amount: u64,
    selection_strategy: *const c_char,
    target_slate_version: i16,
    message: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let mut slate_version: Option<u16> = None;
    if target_slate_version >= 0 {
        slate_version = Some(target_slate_version as u16);
    }

    let res = init_send_tx(
        &cstr_to_str(json_cfg),
        amount,
        &cstr_to_str(selection_strategy),
        slate_version,
        &cstr_to_str(message),
    );
    unsafe { result_to_cstr(res, error) }
}

fn listen(json_cfg: &str) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet = get_wallet_instance(config.clone())?;

    // The streaming channel between 'grinrelay_listener' and 'foreign_listener'
    let (relay_tx_as_payee, relay_rx) = channel();

    // Start a Grin Relay service firstly
    let (grinrelay_key_path, grinrelay_listener) = grinrelay_listener(
        wallet.clone(),
        config.grinrelay_config.clone().unwrap_or_default(),
        None,
        Some(relay_tx_as_payee),
        None,
    )?;

    let _handle = thread::spawn(move || {
        let api = Foreign::new(wallet, None);
        loop {
            match relay_rx.try_recv() {
                Ok((addr, slate)) => {
                    let _slate_id = slate.id;
                    if api.verify_slate_messages(&slate).is_ok() {
                        let slate_rx = api.receive_tx(
                            &slate,
                            Some(&config.account),
                            None,
                            Some(grinrelay_key_path),
                        );
                        if let Ok(slate_rx) = slate_rx {
                            let versioned_slate =
                                VersionedSlate::into_version(slate_rx.clone(), SlateVersion::V2);
                            let res =
                                grinrelay_listener.publish(&versioned_slate, &addr.to_owned());
                            match res {
                                Ok(_) => {
                                    //                                    info!(
                                    //                                        "Slate [{}] sent back to {} successfully",
                                    //                                        slate_id.to_string().bright_green(),
                                    //                                        addr.bright_green(),
                                    //                                    );
                                }
                                Err(_e) => {
                                    //                                    error!(
                                    //                                        "Slate [{}] fail to sent back to {} for {}",
                                    //                                        slate_id.to_string().bright_green(),
                                    //                                        addr.bright_green(),
                                    //                                        e,
                                    //                                    );
                                }
                            }
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            thread::sleep(Duration::from_millis(100));
        }
    });

    //    if handle.is_err() {
    //        Err(ErrorKind::GenericError("Listen thread fail to start".to_string()).into())?
    //    }
    Ok("OK".to_owned())
}

#[no_mangle]
pub extern "C" fn grin_listen(json_cfg: *const c_char, error: *mut u8) -> *const c_char {
    let res = listen(&cstr_to_str(json_cfg));
    unsafe { result_to_cstr(res, error) }
}

fn my_relay_addr(json_cfg: &str) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet = get_wallet_instance(config.clone())?;
    Ok(grinrelay_address(
        wallet.clone(),
        config.grinrelay_config.clone().unwrap_or_default(),
    )?)
}

#[no_mangle]
pub extern "C" fn my_grin_relay_addr(json_cfg: *const c_char, error: *mut u8) -> *const c_char {
    let res = my_relay_addr(&cstr_to_str(json_cfg));
    unsafe { result_to_cstr(res, error) }
}

fn relay_addr_query(json_cfg: &str, six_code_suffix: &str) -> Result<String, Error> {
    let mut is_valid_six_code = false;
    if six_code_suffix.len() == 6 {
        let re = Regex::new(r"[02-9ac-hj-np-z]{6}").unwrap();
        let captures = re.captures(six_code_suffix);
        if captures.is_some() {
            is_valid_six_code = true;
        }
    }
    if !is_valid_six_code {
        return Err(ErrorKind::GenericError("invalid 6-code address".to_owned()).into());
    }

    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet = get_wallet_instance(config.clone())?;

    {
        let (relay_addr_query_sender, relay_addr_query_rx) = channel();

        // Start a Grin Relay service firstly
        let (_key_path, listener) = grinrelay_listener(
            wallet.clone(),
            config.grinrelay_config.clone().unwrap_or_default(),
            None,
            None,
            Some(relay_addr_query_sender),
        )?;

        // Wait for connecting with relay service
        let mut wait_time = 0;
        while !listener.is_connected() {
            thread::sleep(Duration::from_millis(100));
            wait_time += 1;
            if wait_time > 50 {
                return Err(ErrorKind::GenericError(
                    "Fail to connect with grin relay service, 5s timeout. please try again later"
                        .to_owned(),
                )
                .into());
            }
        }

        // Conversion the 6-code abbreviation address to the full address
        {
            let abbr = six_code_suffix.clone();
            if listener.retrieve_relay_addr(abbr.to_string()).is_err() {
                return Err(ErrorKind::GenericError(
                    "Fail to send query request for abbreviated relay addr!".to_owned(),
                )
                .into());
            }

            const TTL: u16 = 10;
            let mut addresses: Option<Vec<String>> = None;
            let mut cnt = 0;
            loop {
                match relay_addr_query_rx.try_recv() {
                    Ok((_abbr, addrs)) => {
                        if !addrs.is_empty() {
                            addresses = Some(addrs);
                        }
                        break;
                    }
                    Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
                cnt += 1;
                if cnt > TTL * 10 {
                    //                    info!(
                    //                        "{} from relay server for address query. {}s timeout",
                    //                        "No response".bright_blue(),
                    //                        TTL
                    //                    );
                    return Err(ErrorKind::GenericError(
                        "relay server no response, please try again later".to_owned(),
                    )
                    .into());
                }
                thread::sleep(Duration::from_millis(100));
            }

            if let Some(addresses) = addresses {
                match addresses.len() {
                    0 => {
                        return Err(ErrorKind::ArgumentError(
                            "wrong address, or destination is offline".to_owned(),
                        )
                        .into());
                    }
                    1 => {
                        let dest = addresses.first().unwrap().clone();
                        return Ok(dest);
                    }
                    _ => {
                        //                        warn!(
                        //                            "{} addresses matched the same abbreviation address: {:?}",
                        //                            addresses.len(),
                        //                            addresses,
                        //                        );
                        return Err(ErrorKind::ArgumentError(
                            "address conflict, multiple matched addresses found".to_owned(),
                        )
                        .into());
                    }
                }
            } else {
                return Err(ErrorKind::ArgumentError(
                    "wrong address, or destination is offline".to_owned(),
                )
                .into());
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn grin_relay_addr_query(
    json_cfg: *const c_char,
    six_code_suffix: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = relay_addr_query(&cstr_to_str(json_cfg), &cstr_to_str(six_code_suffix));
    unsafe { result_to_cstr(res, error) }
}

fn send_tx_by_http(
    json_cfg: &str,
    amount: u64,
    receiver_wallet_url: &str,
    selection_strategy: &str,
    target_slate_version: Option<u16>,
    message: &str,
) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let args = InitTxArgs {
        src_acct_name: None,
        amount,
        minimum_confirmations: SENDING_MINIMUM_CONFIRMATIONS,
        max_outputs: 500,
        num_change_outputs: 1,
        selection_strategy: selection_strategy.to_string(),
        message: Some(message.to_string()),
        target_slate_version,
        estimate_only: None,
        send_args: None,
    };
    let slate_r1 = api.init_send_tx(args)?;

    let adapter = HTTPWalletCommAdapter::new();
    let (slate, _tx_proof) = adapter.send_tx_sync(receiver_wallet_url, &slate_r1)?;
    api.verify_slate_messages(&slate)?;
    api.tx_lock_outputs(&slate_r1, 0)?;

    let finalized_slate = api.finalize_tx(&slate, None, None);
    if finalized_slate.is_err() {
        api.cancel_tx(None, Some(slate_r1.id))?;
    }
    let finalized_slate = finalized_slate?;

    let res = api.post_tx(Some(finalized_slate.id), &finalized_slate.tx, true);
    match res {
        Ok(_) => {
            //info!("Tx sent ok",);
            return Ok(serde_json::to_string(&finalized_slate).expect("fail to serialize slate to json string"));
        }
        Err(e) => {
            // re-post last unconfirmed txs and try again
            if let Ok(true) = api.repost_last_txs(true, false) {
                // iff one re-post success, post this transaction again
                if let Ok(_) = api.post_tx(Some(finalized_slate.id), &finalized_slate.tx, true) {
                    //info!("Tx sent ok (with last unconfirmed tx/s re-post)");
                    return Ok(serde_json::to_string(&finalized_slate).expect("fail to serialize slate to json string"));
                }
            }

            //error!("Tx sent fail on post.");
            let _ = api.cancel_tx(None, Some(finalized_slate.id));
            return Err(ErrorKind::GenericError(e.to_string()).into());
        }
    }
}

fn send_tx_by_relay(
    json_cfg: &str,
    amount: u64,
    receiver_addr: &str,
    selection_strategy: &str,
    target_slate_version: Option<u16>,
    message: &str,
) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet = get_wallet_instance(config.clone())?;
    let api = Owner::new(wallet.clone());
    let args = InitTxArgs {
        src_acct_name: None,
        amount,
        minimum_confirmations: SENDING_MINIMUM_CONFIRMATIONS,
        max_outputs: 500,
        num_change_outputs: 1,
        selection_strategy: selection_strategy.to_string(),
        message: Some(message.to_string()),
        target_slate_version,
        estimate_only: None,
        send_args: None,
    };
    let slate_r1 = api.init_send_tx(args)?;

    // The streaming channel between 'grinrelay_listener' and 'GrinrelayWalletCommAdapter'
    let (relay_tx_as_payer, relay_rx) = channel();

    // Start a Grin Relay service firstly
    let (grinrelay_key_path, grinrelay_listener) = grinrelay_listener(
        wallet.clone(),
        config.grinrelay_config.clone().unwrap_or_default(),
        Some(relay_tx_as_payer),
        None,
        None,
    )?;
    // Wait for connecting with relay service
    let mut wait_time = 0;
    while !grinrelay_listener.is_connected() {
        thread::sleep(Duration::from_millis(100));
        wait_time += 1;
        if wait_time > 50 {
            return Err(ErrorKind::GenericError(
                "Fail to connect with grin relay service, 5s timeout. please try again later"
                    .to_owned(),
            )
                .into());
        }
    }

    let adapter = GrinrelayWalletCommAdapter::new(grinrelay_listener, relay_rx);
    let (slate, tx_proof) = adapter.send_tx_sync(receiver_addr, &slate_r1.clone())?;
    api.verify_slate_messages(&slate)?;
    api.tx_lock_outputs(&slate_r1, 0)?;

    let finalized_slate = api.finalize_tx(&slate, tx_proof, Some(grinrelay_key_path));
    if finalized_slate.is_err() {
        api.cancel_tx(None, Some(slate_r1.id))?;
    }
    let finalized_slate = finalized_slate?;

    let res = api.post_tx(Some(finalized_slate.id), &finalized_slate.tx, true);
    match res {
        Ok(_) => {
            //info!("Tx sent ok",);
            return Ok(serde_json::to_string(&finalized_slate).expect("fail to serialize slate to json string"));
        }
        Err(e) => {
            // re-post last unconfirmed txs and try again
            if let Ok(true) = api.repost_last_txs(true, false) {
                // iff one re-post success, post this transaction again
                if let Ok(_) = api.post_tx(Some(finalized_slate.id), &finalized_slate.tx, true) {
                    //info!("Tx sent ok (with last unconfirmed tx/s re-post)");
                    return Ok(serde_json::to_string(&finalized_slate).expect("fail to serialize slate to json string"));
                }
            }

            //error!("Tx sent fail on post.");
            let _ = api.cancel_tx(None, Some(finalized_slate.id));
            return Err(ErrorKind::GenericError(e.to_string()).into());
        }
    }
}

#[no_mangle]
pub extern "C" fn grin_send_tx(
    json_cfg: *const c_char,
    amount: u64,
    receiver_addr_or_url: *const c_char,
    selection_strategy: *const c_char,
    target_slate_version: i16,
    message: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let mut slate_version: Option<u16> = None;
    if target_slate_version >= 0 {
        slate_version = Some(target_slate_version as u16);
    }

    let receiver = &cstr_to_str(receiver_addr_or_url);
    let res = if receiver.starts_with("http://") || receiver.starts_with("https://") {
        send_tx_by_http(
            &cstr_to_str(json_cfg),
            amount,
            receiver,
            &cstr_to_str(selection_strategy),
            slate_version,
            &cstr_to_str(message),
        )
    } else {
        send_tx_by_relay(
            &cstr_to_str(json_cfg),
            amount,
            receiver,
            &cstr_to_str(selection_strategy),
            slate_version,
            &cstr_to_str(message),
        )
    };
    unsafe { result_to_cstr(res, error) }
}

fn cancel_tx(json_cfg: &str, tx_slate_id: &str) -> Result<String, Error> {
    let uuid = Uuid::parse_str(tx_slate_id).map_err(|e| ErrorKind::GenericError(e.to_string()))?;
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    api.cancel_tx(None, Some(uuid))?;
    Ok("OK".to_owned())
}

#[no_mangle]
pub extern "C" fn grin_cancel_tx(
    json_cfg: *const c_char,
    tx_slate_id: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = cancel_tx(&cstr_to_str(json_cfg), &cstr_to_str(tx_slate_id));
    unsafe { result_to_cstr(res, error) }
}

fn post_tx(json_cfg: &str, tx_slate_id: &str) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let uuid = Uuid::parse_str(tx_slate_id).map_err(|e| ErrorKind::GenericError(e.to_string()))?;
    let (validated, txs) = api.retrieve_txs(true, None, Some(uuid))?;
    if txs[0].confirmed {
        return Err(Error::from(ErrorKind::GenericError(format!(
            "Transaction already confirmed"
        ))));
    } else if !validated {
        return Err(Error::from(ErrorKind::GenericError(format!(
            "api.retrieve_txs not validated"
        ))));
    }

    let stored_tx = api.get_stored_tx(&txs[0])?;
    match stored_tx {
        Some(stored_tx) => {
            api.post_tx(Some(uuid), &stored_tx, true)?;
            Ok("OK".to_owned())
        }
        None => Err(Error::from(ErrorKind::GenericError(format!(
            "transaction data not found"
        )))),
    }
}

#[no_mangle]
pub extern "C" fn grin_post_tx(
    json_cfg: *const c_char,
    tx_slate_id: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = post_tx(&cstr_to_str(json_cfg), &cstr_to_str(tx_slate_id));
    unsafe { result_to_cstr(res, error) }
}

fn tx_file_receive(json_cfg: &str, slate_file_path: &str, message: &str) -> Result<String, Error> {
    let config = MobileWalletCfg::from_str(json_cfg)?;
    let wallet = get_wallet_instance(config.clone())?;
    let api = Foreign::new(wallet, None);
    let adapter = FileWalletCommAdapter::new();
    let mut slate = adapter.receive_tx_async(&slate_file_path)?;
    api.verify_slate_messages(&slate)?;
    slate = api.receive_tx(
        &slate,
        Some(&config.account),
        Some(message.to_string()),
        None,
    )?;
    Ok(serde_json::to_string(&slate).expect("fail to serialize slate to json string"))
}

#[no_mangle]
pub extern "C" fn grin_tx_file_receive(
    json_cfg: *const c_char,
    slate_file_path: *const c_char,
    message: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = tx_file_receive(
        &cstr_to_str(json_cfg),
        &cstr_to_str(slate_file_path),
        &cstr_to_str(message),
    );
    unsafe { result_to_cstr(res, error) }
}

fn tx_file_finalize(json_cfg: &str, slate_file_path: &str) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let adapter = FileWalletCommAdapter::new();
    let mut slate = adapter.receive_tx_async(slate_file_path)?;
    api.verify_slate_messages(&slate)?;
    slate = api.finalize_tx(&slate, None, None)?;
    Ok(serde_json::to_string(&slate).expect("fail to serialize slate to json string"))
}

#[no_mangle]
pub extern "C" fn grin_tx_file_finalize(
    json_cfg: *const c_char,
    slate_file_path: *const c_char,
    error: *mut u8,
) -> *const c_char {
    let res = tx_file_finalize(&cstr_to_str(json_cfg), &cstr_to_str(slate_file_path));
    unsafe { result_to_cstr(res, error) }
}

fn chain_height(json_cfg: &str) -> Result<String, Error> {
    let wallet = get_wallet_instance(MobileWalletCfg::from_str(json_cfg)?)?;
    let api = Owner::new(wallet);
    let height = api.node_height()?;
    Ok(serde_json::to_string(&height).unwrap())
}

#[no_mangle]
pub extern "C" fn grin_chain_height(json_cfg: *const c_char, error: *mut u8) -> *const c_char {
    let res = chain_height(&cstr_to_str(json_cfg));
    unsafe { result_to_cstr(res, error) }
}
