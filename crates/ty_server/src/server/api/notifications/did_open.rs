use lsp_types::notification::DidOpenTextDocument;
use lsp_server::ErrorCode;
use lsp_types::{
    notification::{PublishDiagnostics},
    DidOpenTextDocumentParams,
    PublishDiagnosticsParams,
    TextDocumentItem
};

use ruff_db::Db;
use ty_project::watch::ChangeEvent;

use crate::server::api::LSPResult;
use crate::server::api::traits::{NotificationHandler, SyncNotificationHandler};
use crate::server::api::diagnostics::compute_diagnostics;
use crate::server::client::{Notifier, Requester};
use crate::server::Result;
use crate::session::Session;
use crate::system::{url_to_any_system_path, AnySystemPath};
use crate::TextDocument;

pub(crate) struct DidOpenTextDocumentHandler;

impl NotificationHandler for DidOpenTextDocumentHandler {
    type NotificationType = DidOpenTextDocument;
}

impl SyncNotificationHandler for DidOpenTextDocumentHandler {
    fn run(
        session: &mut Session,
        notifier: Notifier,
        _requester: &mut Requester,
        DidOpenTextDocumentParams {
            text_document:
                TextDocumentItem {
                    uri,
                    text,
                    version,
                    language_id,
                },
        }: DidOpenTextDocumentParams,
    ) -> Result<()> {
        let Ok(path) = url_to_any_system_path(&uri) else {
            return Ok(());
        };

        let document = TextDocument::new(text, version).with_language_id(&language_id);
        let key = session.key_from_url(uri.clone());
        session.open_text_document(uri.clone(), document);

        let should_compute_diagnostics = !session.client_capabilities().pull_diagnostics;
        
        // Get the snapshot before getting mutable db reference
        let snapshot = if should_compute_diagnostics {
            Some(session.take_snapshot(uri.clone())
                .expect("Document should exist after opening"))
        } else {
            None
        };

        let db = match path {
            AnySystemPath::System(path) => {
                let db = match session.project_db_for_path_mut(path.as_std_path()) {
                    Some(db) => db,
                    None => session.default_project_db_mut(),
                };
                db.apply_changes(vec![ChangeEvent::Opened(path)], None);
                db
            }
            AnySystemPath::SystemVirtual(virtual_path) => {
                let db = session.default_project_db_mut();
                db.files().virtual_file(db, &virtual_path);
                db
            }
        };

        if let Some(snapshot) = snapshot {
            let diagnostics = compute_diagnostics(&snapshot, db);
            notifier
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: Some(version),
                })
                .with_failure_code(ErrorCode::InternalError)?;
        }

        Ok(())
    }
}