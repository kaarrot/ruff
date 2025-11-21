use std::borrow::Cow;
use lsp_types::request::References;
use lsp_types::{Location, ReferenceParams, Url};
use ruff_db::source::{line_index, source_text};
use ty_ide::find_references_with_files;
use ty_project::{Db, ProjectDatabase};

use crate::document::{PositionExt, ToLink};
use crate::server::api::traits::{BackgroundDocumentRequestHandler, RequestHandler};
use crate::session::client::Client;
use crate::DocumentSnapshot;

pub(crate) struct ReferencesRequestHandler;

impl RequestHandler for ReferencesRequestHandler {
    type RequestType = References;
}

impl BackgroundDocumentRequestHandler for ReferencesRequestHandler {
    fn document_url(params: &ReferenceParams) -> Cow<Url> {
        Cow::Borrowed(&params.text_document_position.text_document.uri)
    }

    fn run_with_snapshot(
        db: &ProjectDatabase,
        snapshot: DocumentSnapshot,
        _client: &Client,
        params: ReferenceParams,
    ) -> crate::server::Result<Option<Vec<Location>>> {
        let Some(file) = snapshot.file(db) else {
            tracing::debug!("Failed to resolve file for {:?}", params);
            return Ok(None);
        };

        let source = source_text(db, file);
        let line_index = line_index(db, file);
        let offset = params.text_document_position.position.to_text_size(
            &source,
            &line_index,
            snapshot.encoding(),
        );

        let include_declaration = params.context.include_declaration;

        // Get all files from the project to search across modules
        let project = db.project();
        let all_files: Vec<_> = project.files(db).into_iter().collect();

        tracing::debug!("Searching for references across {} files", all_files.len());

        let Some(references) = find_references_with_files(
            db,
            file,
            offset,
            include_declaration,
            all_files.into_iter(),
        ) else {
            return Ok(None);
        };

        let locations: Vec<_> = references
            .into_iter()
            .filter_map(|target| target.to_location(db, snapshot.encoding()))
            .collect();

        Ok(Some(locations))
    }
}
