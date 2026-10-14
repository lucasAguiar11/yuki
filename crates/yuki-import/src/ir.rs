/// A parsed collection ready to convert. `items` preserves source order; the
/// converter assigns ids/timestamps and wires `folder_id` from the tree.
#[derive(Debug, Clone, Default)]
pub struct ImportedCollection {
    pub name: String,
    /// Source spec version (e.g. `3.0.3`), shown in the preview subtitle.
    pub version: Option<String>,
    /// Collection/base variables → the workspace's Global environment.
    pub variables: Vec<ImportedVariable>,
    /// Named sub-environments (dev/staging/…) → one Voleeo environment each.
    pub environments: Vec<ImportedEnvironment>,
    /// Collection-level / global security → flattened onto every request.
    pub root_auth: ImportedAuth,
    /// Collection/workspace headers → merged under every request's own headers.
    pub root_headers: Vec<ImportedParam>,
    pub items: Vec<ImportedItem>,
    /// Unsupported features surfaced to the user (skipped scripts, external refs…).
    pub warnings: Vec<String>,
}

/// A named environment from the source (Insomnia sub-env, Yaak sub-env, Bruno
/// `config.environments` entry) → its own Voleeo environment on import.
#[derive(Debug, Clone)]
pub struct ImportedEnvironment {
    pub name: String,
    pub variables: Vec<ImportedVariable>,
}

#[derive(Debug, Clone)]
pub enum ImportedItem {
    Folder(ImportedFolder),
    Request(ImportedRequest),
}

#[derive(Debug, Clone, Default)]
pub struct ImportedFolder {
    pub name: String,
    /// Tag/group description, shown beside the folder name in the preview.
    pub description: Option<String>,
    pub auth: ImportedAuth,
    pub headers: Vec<ImportedParam>,
    pub variables: Vec<ImportedVariable>,
    pub items: Vec<ImportedItem>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportedRequest {
    pub name: String,
    pub method: String,
    /// URL with `:name` path-param segments and `{{ base_url }}`-style prefixes.
    pub url: String,
    /// Raw source path (`/pet/{petId}`) for display/search in the preview.
    pub path: String,
    /// Path params (`:name`) — empty value, the user fills them in.
    pub path_params: Vec<ImportedParam>,
    pub query: Vec<ImportedParam>,
    pub headers: Vec<ImportedParam>,
    pub body: Option<ImportedBody>,
    pub auth: ImportedAuth,
}

#[derive(Debug, Clone)]
pub struct ImportedParam {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ImportedVariable {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum ImportedBody {
    Raw {
        hint: RawKind,
        text: String,
    },
    FormUrlEncoded(Vec<ImportedField>),
    Multipart(Vec<ImportedField>),
    /// The file path (empty when the source has none).
    Binary(String),
    GraphQl {
        query: String,
        variables: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum RawKind {
    Json,
    Xml,
    Text,
    Html,
}

#[derive(Debug, Clone)]
pub struct ImportedField {
    pub name: String,
    /// For `is_file`, the file path.
    pub value: String,
    pub is_file: bool,
    pub enabled: bool,
    pub content_type: Option<String>,
}

/// Auth schemes the IR can carry. `convert` maps these onto
/// `yuki_core::AuthConfig`. Absent auth is `Inherit` (resolves to the enclosing
/// scope, `None` at the root); an explicit `None` stops inheritance;
/// `Unsupported` becomes `None` plus a warning. Schemes with many fields
/// (OAuth1/2, AWS, Digest, NTLM) go straight to core as `Core`.
// Transient (one value per imported item): boxing `Core` would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default)]
pub enum ImportedAuth {
    None,
    #[default]
    Inherit,
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    ApiKey {
        key: String,
        value: String,
        in_header: bool,
    },
    Core(yuki_core::AuthConfig),
    Unsupported(String),
}
