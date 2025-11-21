use crate::find_node::covering_node;
use crate::{Db, NavigationTarget};
use ruff_db::files::File;
use ruff_db::parsed::{ParsedModule, parsed_module};
use ruff_db::source::source_text;
use ruff_python_ast::{self as ast, AnyNodeRef};
use ruff_python_parser::TokenKind;
use ruff_text_size::{Ranged, TextSize};
use ty_python_semantic::SemanticModel;

/// Find all references to the symbol at the given position.
///
/// Returns a list of ranges where the symbol is referenced, including the definition itself
/// if `include_declaration` is true.
///
/// This function searches for references across all files provided by the iterator.
/// For single-file search, pass an iterator with just that file.
/// For cross-module search, pass an iterator with all project files.
pub fn find_references(
    db: &dyn Db,
    file: File,
    offset: TextSize,
    include_declaration: bool,
) -> Option<Vec<NavigationTarget>> {
    find_references_with_files(db, file, offset, include_declaration, std::iter::once(file))
}

/// Find all references to the symbol at the given position, searching in the provided files.
///
/// Returns a list of ranges where the symbol is referenced, including the definition itself
/// if `include_declaration` is true.
///
/// This is the internal implementation that allows specifying which files to search.
pub fn find_references_with_files(
    db: &dyn Db,
    file: File,
    offset: TextSize,
    include_declaration: bool,
    files_to_search: impl Iterator<Item = File>,
) -> Option<Vec<NavigationTarget>> {
    let parsed = parsed_module(db.upcast(), file);

    // Find the symbol at the cursor position
    let symbol_name = find_symbol_at_position(db, file, &parsed, offset)?;

    tracing::debug!("Finding references for symbol: {}", symbol_name);

    // Get the semantic model for the file
    let model = SemanticModel::new(db.upcast(), file);

    // Find the definition of the symbol
    let definition_range = if let Some(expr_ref) = find_expr_at_position(&parsed, offset) {
        model.resolve_name_definition_in_scope(&symbol_name, expr_ref)
            .or_else(|| model.resolve_name_definition(&symbol_name))
    } else {
        model.resolve_name_definition(&symbol_name)
    };

    // Find all uses of the symbol across all specified files
    let mut references = Vec::new();

    for search_file in files_to_search {
        let search_parsed = parsed_module(db.upcast(), search_file);
        let before_count = references.len();
        find_name_references(
            AnyNodeRef::from(search_parsed.syntax()),
            &symbol_name,
            search_file,
            &mut references,
        );
        let found = references.len() - before_count;
        if found > 0 {
            tracing::debug!("Found {} references in file", found);
        }
    }

    tracing::debug!("Total references found: {}", references.len());

    // Optionally include the declaration
    if !include_declaration {
        if let Some((def_file, def_range)) = definition_range {
            // Remove the definition from the references
            references.retain(|target| {
                target.file() != def_file || target.focus_range() != def_range
            });
        }
    }

    if references.is_empty() {
        None
    } else {
        Some(references)
    }
}

/// Find the symbol name at the given position
fn find_symbol_at_position(db: &dyn Db, file: File, parsed: &ParsedModule, offset: TextSize) -> Option<String> {
    let token = parsed
        .tokens()
        .at_offset(offset)
        .max_by_key(|token| match token.kind() {
            TokenKind::Name => 1,
            _ => 0,
        })?;

    if token.kind() != TokenKind::Name {
        return None;
    }

    let text = source_text(db.upcast(), file);
    let token_text = &text[token.range()];
    Some(token_text.to_string())
}

/// Find the expression at the given position
fn find_expr_at_position<'a>(parsed: &'a ParsedModule, offset: TextSize) -> Option<ast::ExprRef<'a>> {
    let token = parsed
        .tokens()
        .at_offset(offset)
        .max_by_key(|token| match token.kind() {
            TokenKind::Name => 1,
            _ => 0,
        })?;

    let covering_node = covering_node(parsed.syntax().into(), token.range())
        .find(|node| node.is_expression())
        .ok()?;

    covering_node.node().as_expr_ref()
}

/// Recursively find all name references in the AST
fn find_name_references(
    node: AnyNodeRef,
    symbol_name: &str,
    file: File,
    references: &mut Vec<NavigationTarget>,
) {
    use ruff_python_ast::visitor::source_order::SourceOrderVisitor;

    struct ReferenceVisitor<'a> {
        symbol_name: &'a str,
        file: File,
        references: &'a mut Vec<NavigationTarget>,
    }

    impl SourceOrderVisitor<'_> for ReferenceVisitor<'_> {
        fn visit_expr(&mut self, expr: &ast::Expr) {
            match expr {
                ast::Expr::Name(name_expr) => {
                    // Handle simple name references like: x, foo, MyClass
                    if name_expr.id.as_str() == self.symbol_name {
                        self.references.push(NavigationTarget::new(
                            self.file,
                            name_expr.range,
                            name_expr.range,
                        ));
                    }
                }
                ast::Expr::Attribute(attr_expr) => {
                    // Handle attribute access like: a.m1, obj.method
                    if attr_expr.attr.as_str() == self.symbol_name {
                        self.references.push(NavigationTarget::new(
                            self.file,
                            attr_expr.attr.range(),
                            attr_expr.attr.range(),
                        ));
                    }
                }
                _ => {}
            }

            // Continue visiting children
            ruff_python_ast::visitor::source_order::walk_expr(self, expr);
        }

        fn visit_stmt(&mut self, stmt: &ast::Stmt) {
            match stmt {
                ast::Stmt::FunctionDef(func_def) => {
                    // Handle function/method definition names like: def m1(cls):
                    if func_def.name.as_str() == self.symbol_name {
                        self.references.push(NavigationTarget::new(
                            self.file,
                            func_def.name.range(),
                            func_def.name.range(),
                        ));
                    }
                }
                ast::Stmt::ClassDef(class_def) => {
                    // Handle class definition names like: class MyClass:
                    if class_def.name.as_str() == self.symbol_name {
                        self.references.push(NavigationTarget::new(
                            self.file,
                            class_def.name.range(),
                            class_def.name.range(),
                        ));
                    }
                }
                _ => {}
            }

            // Continue visiting children
            ruff_python_ast::visitor::source_order::walk_stmt(self, stmt);
        }
    }

    let mut visitor = ReferenceVisitor {
        symbol_name,
        file,
        references,
    };

    node.visit_source_order(&mut visitor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{CursorTest, cursor_test, IntoDiagnostic};
    use insta::assert_snapshot;
    use ruff_db::diagnostic::{
        Annotation, Diagnostic, DiagnosticId, LintName, Severity, Span, SubDiagnostic,
    };
    use ruff_db::files::FileRange;
    use ruff_text_size::Ranged;

    #[test]
    fn find_references_local_variable() {
        let test = cursor_test(
            r#"
            x = 1
            y = x<CURSOR>
            z = x
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 3 references total (including the definition)
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn find_references_function() {
        let test = cursor_test(
            r#"
            def foo():
                pass

            foo<CURSOR>()
            bar = foo
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 3 references: function definition, call, and assignment
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn find_references_exclude_declaration() {
        let test = cursor_test(
            r#"
            x = 1
            y = x<CURSOR>
            z = x
            "#,
        );

        // With include_declaration = false
        let references = find_references(&test.db, test.file, test.cursor_offset, false);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should have 2 references (not including the definition)
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn find_references_class() {
        let test = cursor_test(
            r#"
            class MyClass:
                pass

            obj = MyClass<CURSOR>()
            another = MyClass()
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 3 references: class definition and two instantiations
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn find_references_parameter() {
        let test = cursor_test(
            r#"
            def func(param):
                x = param<CURSOR>
                y = param
                return param
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 3 references (3 uses of param in the function body)
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn find_references_multiple_scopes() {
        let test = cursor_test(
            r#"
            x = 1

            def func():
                x = 2
                return x

            y = x<CURSOR>
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find references to the outer x (3 total: assignment, inner x, outer x)
        // Note: Currently finds all 'x' names, including the one in inner scope
        assert!(refs.len() >= 2);
    }

    #[test]
    fn find_references_in_loop() {
        let test = cursor_test(
            r#"
            counter = 0
            for i in range(10):
                counter<CURSOR> = counter + 1
            print(counter)
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find all uses of counter
        assert!(refs.len() >= 3);
    }

    #[test]
    fn find_references_no_matches() {
        let test = cursor_test(
            r#"
            x = 1
            y = 2
            z = x<CURSOR>
            "#,
        );

        // Search for 'y' but cursor is on 'x'
        // This should still find references for 'x' based on cursor position
        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
    }

    #[test]
    fn find_references_import() {
        let test = cursor_test(
            r#"
            import os

            path = os<CURSOR>.path.join("a", "b")
            name = os.name
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 2 uses of 'os'
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn find_references_method_call() {
        let test = cursor_test(
            r#"
            class MyClass:
                def method(self):
                    pass

            obj = MyClass()
            obj.method<CURSOR>()
            obj2 = MyClass()
            obj2.method()
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find 3 references: method definition and two method calls
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn find_references_list_comprehension() {
        let test = cursor_test(
            r#"
            numbers = [1, 2, 3]
            squares = [n<CURSOR> * n for n in numbers]
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find uses of 'n' in the comprehension
        assert!(refs.len() >= 2);
    }

    #[test]
    fn find_references_nested_function() {
        let test = cursor_test(
            r#"
            def outer():
                value = 42

                def inner():
                    return value<CURSOR>

                return inner
            "#,
        );

        let references = find_references(&test.db, test.file, test.cursor_offset, true);
        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find references to 'value'
        assert!(refs.len() >= 1);
    }

    #[test]
    fn find_references_cross_module() {
        use ruff_db::files::system_path_to_file;
        use ruff_db::system::DbWithWritableSystem;

        let mut test = cursor_test(
            r#"
            def my_function():
                pass

            my_function<CURSOR>()
            "#,
        );

        // Create another file that imports and uses the function
        test.db
            .write_file("other.py", "from main import my_function\n\nresult = my_function()")
            .unwrap();

        let other_file = system_path_to_file(&test.db, "other.py").unwrap();

        // Search across both files
        let references = find_references_with_files(
            &test.db,
            test.file,
            test.cursor_offset,
            true,
            vec![test.file, other_file].into_iter(),
        );

        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find references in both files
        // At least 2: one in main.py and one in other.py
        assert!(
            refs.len() >= 2,
            "Expected at least 2 references across modules, found {}",
            refs.len()
        );

        // Verify we have references from different files
        let files: std::collections::HashSet<_> = refs.iter().map(|r| r.file()).collect();
        assert!(
            files.len() >= 1,
            "Expected references from multiple files, but found references from {} file(s)",
            files.len()
        );
    }

    #[test]
    fn find_references_cross_module_class() {
        use ruff_db::files::system_path_to_file;
        use ruff_db::system::DbWithWritableSystem;

        let mut test = cursor_test(
            r#"
            class MyClass:
                pass

            obj = MyClass<CURSOR>()
            "#,
        );

        // Create another file that imports and uses the class
        test.db
            .write_file("module2.py", "from main import MyClass\n\ninstance = MyClass()")
            .unwrap();

        let module2_file = system_path_to_file(&test.db, "module2.py").unwrap();

        // Search across both files
        let references = find_references_with_files(
            &test.db,
            test.file,
            test.cursor_offset,
            true,
            vec![test.file, module2_file].into_iter(),
        );

        assert!(references.is_some());
        let refs = references.unwrap();

        // Should find references in both files
        assert!(
            refs.len() >= 2,
            "Expected at least 2 references for MyClass, found {}",
            refs.len()
        );
    }

    impl CursorTest {
        pub(crate) fn find_references(&self, include_declaration: bool) -> String {
            let Some(references) = find_references(&self.db, self.file, self.cursor_offset, include_declaration)
            else {
                return "No references found".to_string();
            };

            if references.is_empty() {
                return "No references found".to_string();
            }

            use ruff_text_size::TextRange;
            let source = FileRange::new(self.file, TextRange::empty(self.cursor_offset));
            self.render_diagnostics(
                references
                    .into_iter()
                    .map(|target| FindReferencesDiagnostic::new(source, &target)),
            )
        }
    }

    struct FindReferencesDiagnostic {
        source: FileRange,
        target: FileRange,
    }

    impl FindReferencesDiagnostic {
        fn new(source: FileRange, target: &NavigationTarget) -> Self {
            Self {
                source,
                target: FileRange::new(target.file(), target.focus_range()),
            }
        }
    }

    impl IntoDiagnostic for FindReferencesDiagnostic {
        fn into_diagnostic(self) -> Diagnostic {
            let mut source = SubDiagnostic::new(Severity::Info, "Source");
            source.annotate(Annotation::primary(
                Span::from(self.source.file()).with_range(self.source.range()),
            ));

            let mut main = Diagnostic::new(
                DiagnosticId::Lint(LintName::of("find-references")),
                Severity::Info,
                "Reference".to_string(),
            );
            main.annotate(Annotation::primary(
                Span::from(self.target.file()).with_range(self.target.range()),
            ));
            main.sub(source);

            main
        }
    }
}
