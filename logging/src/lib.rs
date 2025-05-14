use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};

use crate::hyperware::process::logging::Request as LoggingRequest;
use hyperware_process_lib::logging::{error, info, init_logging, warn, Level};
use hyperware_process_lib::vfs::{create_drive, open_dir, open_file, File};
use hyperware_process_lib::{
    await_message, call_init, get_state, set_state, Address, Message, PackageId,
};

wit_bindgen::generate!({
    path: "../target/wit",
    world: "logging-nick-dot-hypr-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

#[derive(Debug, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto)]
#[serde(untagged)] // untagged as a meta-type for all incoming messages
enum Req {
    LoggingRequest(LoggingRequest),
    InternalRequest(InternalRequest),
}

type Packages = HashSet<PackageId>;
type Nodes = HashSet<String>;
type Files = HashMap<Address, File>;

#[derive(Debug, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto)]
enum InternalRequest {
    AddAllowedPackage(PackageId),
    RemoveAllowedPackage(PackageId),
    WhitelistNode(String),
    UnwhitelistNode(String),
    BlacklistNode(String),
    UnblacklistNode(String),
}

/// drive_path       : populated at process init()
/// log_files        : added to over the run of the program to reduce number of VFS calls
/// allowed_packages : packages to log for; empty -> all
/// whitelist        : nodes to log for; empty -> all
/// blacklist        : nodes to NOT log for; empty -> all
#[derive(serde::Deserialize, serde::Serialize)]
struct State {
    drive_path: String,
    log_files: Files,
    allowed_packages: Packages,
    whitelist: Nodes,
    blacklist: Nodes,
}

impl State {
    fn new(drive_path: String) -> Self {
        Self {
            drive_path,
            log_files: HashMap::new(),
            allowed_packages: HashSet::new(),
            whitelist: HashSet::new(),
            blacklist: HashSet::new(),
        }
    }

    fn load(drive_path: String) -> Self {
        match get_state() {
            None => Self::new(drive_path),
            Some(ref state_bytes) => match serde_json::from_slice(state_bytes) {
                Ok(state) => state,
                Err(e) => {
                    warn!("failed to deserialize saved state: {e:?}");
                    Self::new(drive_path)
                }
            },
        }
    }

    fn save(&self) {
        match serde_json::to_vec(self) {
            Ok(ref state_bytes) => set_state(state_bytes),
            Err(e) => error!("failed to serialize state: {e:?}"),
        }
    }
}

/// check if node is on whitelist (if it exists) & not on blacklist (if it exists)
///
/// return value of None -> node is allowed;
/// return value of Some -> node is not allowed (and an explanatory message)
fn is_node_allowed(source: &Address, state: &State) -> Option<String> {
    if !state.whitelist.is_empty() && !state.whitelist.contains(source.node()) {
        return Some(format!(
            "dropping log Request from un-whitelisted node {}",
            source.node(),
        ));
    }
    if !state.blacklist.is_empty() && state.blacklist.contains(source.node()) {
        return Some(format!(
            "dropping log Request from blacklisted node {}",
            source.node(),
        ));
    }
    None
}

/// check if node is on whitelist (if it exists) & not on blacklist (if it exists)
///
/// return value of None -> node is allowed;
/// return value of Some -> node is not allowed (and an explanatory message)
fn is_package_allowed(source: &Address, state: &State) -> Option<String> {
    if !state.allowed_packages.is_empty() && !state.allowed_packages.contains(&source.package_id())
    {
        Some(format!(
            "dropping log Request from package {}; not amongst allowed packages: {:?}",
            source.package_id(),
            state.allowed_packages,
        ))
    } else {
        None
    }
}

fn handle_logging_request(
    source: &Address,
    request: &LoggingRequest,
    state: &mut State,
) -> Result<()> {
    match request {
        LoggingRequest::Log(ref log) => {
            let number_log_files = state.log_files.len();

            let mut log: serde_json::Value = serde_json::from_str(log)?;
            log["source"] = serde_json::json!(source);
            let mut log = serde_json::to_vec(&log).unwrap();
            log.push(10); // add `\n`
                          //
            let log_file = state.log_files.entry(source.clone()).or_insert_with(|| {
                let log_dir_path = format!("{}/{}", state.drive_path, source.package_id());
                let _log_dir = open_dir(&log_dir_path, true, None).expect("failed to open log dir");
                let log_file_path = format!("{log_dir_path}/{}.log", source.process());
                open_file(&log_file_path, true, None).expect("failed to open log file")
            });
            log_file.append(&log)?;

            if number_log_files != state.log_files.len() {
                state.save();
            }
        }
    }
    Ok(())
}

fn handle_internal_request(
    our: &Address,
    source: &Address,
    request: InternalRequest,
    state: &mut State,
) -> Result<()> {
    if our != source {
        return Err(anyhow!(
            "rejecting InternalRequest from remote node {source}"
        ));
    }
    match request {
        InternalRequest::AddAllowedPackage(p) => state.allowed_packages.insert(p),
        InternalRequest::RemoveAllowedPackage(ref p) => state.allowed_packages.remove(p),
        InternalRequest::WhitelistNode(node) => state.whitelist.insert(node),
        InternalRequest::UnwhitelistNode(ref node) => state.whitelist.remove(node),
        InternalRequest::BlacklistNode(node) => state.blacklist.insert(node),
        InternalRequest::UnblacklistNode(ref node) => state.blacklist.remove(node),
    };
    state.save();
    Ok(())
}

fn handle_message(our: &Address, message: &Message, state: &mut State) -> Result<()> {
    if !message.is_request() {
        return Ok(());
    }
    let source = message.source();
    if let Some(ref failure_message) = is_node_allowed(source, state) {
        info!("{failure_message}");
        return Ok(());
    }
    if let Some(ref failure_message) = is_package_allowed(source, state) {
        info!("{failure_message}");
        return Ok(());
    }

    match message.body().try_into()? {
        Req::LoggingRequest(ref request) => handle_logging_request(source, request, state)?,
        Req::InternalRequest(request) => handle_internal_request(our, source, request, state)?,
    }
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    init_logging(Level::DEBUG, Level::INFO, None, None, None).unwrap();
    info!("begin");
    let drive_path = create_drive(our.package_id(), "remote-log", None).unwrap();

    let mut state = State::load(drive_path);

    loop {
        match await_message() {
            Err(send_error) => error!("got SendError: {send_error}"),
            Ok(ref message) => {
                if let Err(e) = handle_message(&our, message, &mut state) {
                    error!("got error while handling message: {e:?}");
                }
            }
        }
    }
}
