use lsp_server::ErrorCode;
use lsp_types::notification::{DidChangeTextDocument, PublishDiagnostics};
use lsp_types::{DidChangeTextDocumentParams, PublishDiagnosticsParams};

use ty_project::watch::ChangeEvent;

use crate::server::api::traits::{NotificationHandler, SyncNotificationHandler};
use crate::server::api::diagnostics::compute_diagnostics;
use crate::server::client::{Notifier, Requester};
use crate::server::Result;
use crate::session::Session;
use crate::system::{url_to_any_system_path, AnySystemPath};
use crate::server::api::LSPResult;

pub(crate) struct DidChangeTextDocumentHandler;

impl NotificationHandler for DidChangeTextDocumentHandler {
    type NotificationType = DidChangeTextDocument;
}

impl SyncNotificationHandler for DidChangeTextDocumentHandler {
    fn run(
        session: &mut Session,
        notifier: Notifier,
        _requester: &mut Requester,
        params: DidChangeTextDocumentParams,
    ) -> Result<()> {
        let Ok(path) = url_to_any_system_path(&params.text_document.uri) else {
            return Ok(());
        };

        let key = session.key_from_url(params.text_document.uri.clone());
        let should_compute_diagnostics = !session.client_capabilities().pull_diagnostics;

        session
            .update_text_document(&key, params.content_changes, params.text_document.version)
            .with_failure_code(ErrorCode::InternalError)?;

        // Get the snapshot before getting mutable db reference
        let snapshot = if should_compute_diagnostics {
            Some(session.take_snapshot(params.text_document.uri.clone())
                .expect("Document should exist after update"))
        } else {
            None
        };

        let db = match &path {
            AnySystemPath::System(path) => {
                let db = match session.project_db_for_path_mut(path.as_std_path()) {
                    Some(db) => db,
                    None => session.default_project_db_mut(),
                };
                db.apply_changes(vec![ChangeEvent::file_content_changed(path.clone())], None);
                db
            }
            AnySystemPath::SystemVirtual(virtual_path) => {
                let db = session.default_project_db_mut();
                db.apply_changes(vec![ChangeEvent::ChangedVirtual(virtual_path.clone())], None);
                db
            }
        };

        if let Some(snapshot) = snapshot {
            let diagnostics = compute_diagnostics(&snapshot, db);
            notifier
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri: params.text_document.uri,
                    diagnostics,
                    version: Some(params.text_document.version),
                })
                .with_failure_code(ErrorCode::InternalError)?;
        }

        Ok(())
    }
}