//! Shared domain primitives and common pages.
//!
//! Scope: re-exports of the shared value types (`src/structs/common`), shared
//! extractors, and common pages such as index and language switching.
mod extractors;
mod forms;
mod pages;
mod paths;

pub use forms::{
    DutchAddressForm, FullNameForm, InternationalAddressForm, MinimalNameForm, SelectElectionForm,
    SwitchElectionForm,
};

pub use pages::{
    always_public_router, auth_failure_response, not_found, public_router, router,
    session_only_router,
};
pub use paths::{
    HideDownloadWarningPath, LoginStartPath, LogoutPath, PgIndexPath, SelectElectionPath,
    SessionExpiryPath, SwitchElectionPath, SwitchLanguagePath,
};
