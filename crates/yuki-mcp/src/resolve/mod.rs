//! Backend mirror of `sendResolution.ts`, shared by the Tauri commands and MCP
//! tools. `vars` loads env/folder variables and resolves `{{ VAR }}` tokens and
//! `funcs` (`{{ $fake.email() }}`); `responses` prefills `$response.body(…)`
//! from stored responses; `http` applies them to HTTP/WS requests (+ auth);
//! `grpc` does the gRPC equivalent; `text` holds the pure URL/encoding
//! helpers. Unknown function tokens pass through verbatim.

pub mod funcs;
mod global;
mod grpc;
mod http;
mod responses;
mod system_env;
mod text;
mod vars;

pub use global::{
    set_settings as set_global_settings, settings as global_settings, GlobalSettings,
};
pub use grpc::{apply_to_grpc, grpc_vars, resolve_grpc_for_send};
pub(crate) use http::auth_header;
pub use http::{apply_to_connection, apply_to_request};
pub use responses::{
    args as response_args, body as response_value, prefill_responses, select as select_json,
    sources as response_sources,
};
pub use system_env::{
    allowlist as system_env_allowlist, effective_skip_cert_check,
    set_allowlist as set_system_env_allowlist, set_skip_cert_check, skip_cert_check,
    snapshot as system_env_snapshot,
};
pub use text::path_params;
pub use vars::{
    apply_folder_vars, effective_auth, is_identifier, load_env_vars, load_env_vars_from,
    merge_inherited_metadata, resolve_str,
};
