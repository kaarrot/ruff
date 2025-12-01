use ruff_db::files::{File, FilePath};
use ruff_db::source::line_index;
use ruff_python_ast as ast;
use ruff_python_ast::{Expr, ExprRef, name::Name};
use ruff_source_file::LineIndex;
use ruff_text_size::TextRange;
use ruff_text_size::Ranged;

use crate::Db;
use crate::module_name::ModuleName;
use crate::module_resolver::{Module, resolve_module};
use crate::semantic_index::ast_ids::HasScopedExpressionId;
use crate::semantic_index::semantic_index;
use crate::semantic_index::symbol::FileScopeId;
use crate::types::{Type, binding_type, infer_scope_types};

pub struct SemanticModel<'db> {
    db: &'db dyn Db,
    file: File,
}

impl<'db> SemanticModel<'db> {
    pub fn new(db: &'db dyn Db, file: File) -> Self {
        Self { db, file }
    }

    // TODO we don't actually want to expose the Db directly to lint rules, but we need to find a
    // solution for exposing information from types
    pub fn db(&self) -> &dyn Db {
        self.db
    }

    pub fn file_path(&self) -> &FilePath {
        self.file.path(self.db)
    }

    pub fn line_index(&self) -> LineIndex {
        line_index(self.db.upcast(), self.file)
    }

    pub fn resolve_module(&self, module_name: &ModuleName) -> Option<Module> {
        resolve_module(self.db, module_name)
    }

    /// Resolves a module from an import statement to its file location.
    /// Handles both absolute and relative imports.
    pub fn resolve_import_module_definition(
        &self,
        import_from: &ast::StmtImportFrom,
    ) -> Option<(File, TextRange)> {
        // Resolve the module name (handles both absolute and relative imports)
        let module_name = crate::module_name::ModuleName::from_import_statement(
            self.db,
            self.file,
            import_from
        ).ok()?;

        let module = self.resolve_module(&module_name)?;
        let file = module.file()?;

        // Return the first line of the file (or a better location if we can find it)
        Some((file, TextRange::default()))
    }

    /// Resolves a variable/function/class name to its definition location in the current file.
    /// Searches starting from the global scope.
    pub fn resolve_name_definition(
        &self,
        name: &str,
    ) -> Option<(File, TextRange)> {
        let index = semantic_index(self.db, self.file);
        // Find the binding for the given name in the current file
        let binding = index.binding_by_name(name)?;

        // Check if this binding is an import - if so, follow the import chain
        use crate::semantic_index::definition::DefinitionKind;
        match binding.kind(self.db) {
            DefinitionKind::ImportFrom(import_from) => {
                return self.resolve_imported_symbol_definition(import_from);
            }
            _ => {
                // For non-import bindings, return the local definition
                let range = binding.focus_range(self.db).range();
                Some((self.file, range))
            }
        }
    }

    /// Resolves a variable/function/class name to its definition location,
    /// searching from a specific expression's scope and walking up the scope chain.
    /// Returns the first definition in source order.
    pub fn resolve_name_definition_in_scope(
        &self,
        name: &str,
        expr: ast::ExprRef<'_>,
    ) -> Option<(File, TextRange)> {
        let index = semantic_index(self.db, self.file);
        let file_scope = index.expression_scope_id(expr);

        // Walk up the scope chain looking for the symbol
        for (scope_id, _scope) in index.ancestor_scopes(file_scope) {
            let symbol_table = index.symbol_table(scope_id);
            if let Some(symbol_id) = symbol_table.symbol_id_by_name(name) {
                let use_def = index.use_def_map(scope_id);

                // Collect all definitions (including shadowed ones) and find the first one in source order
                let mut first_binding: Option<(crate::semantic_index::definition::Definition, TextRange)> = None;

                for definition in use_def.all_definitions_for_symbol(self.db, symbol_id) {
                    let range = definition.focus_range(self.db).range();

                    // Keep the binding with the smallest start offset (earliest in source)
                    if let Some((_, first_range)) = first_binding {
                        if range.start() < first_range.start() {
                            first_binding = Some((definition, range));
                        }
                    } else {
                        first_binding = Some((definition, range));
                    }
                }

                if let Some((binding, range)) = first_binding {
                    // Check if this binding is an import - if so, follow the import chain
                    use crate::semantic_index::definition::DefinitionKind;
                    match binding.kind(self.db) {
                        DefinitionKind::ImportFrom(import_from) => {
                            return self.resolve_imported_symbol_definition(import_from);
                        }
                        _ => {
                            // For non-import bindings, return the local definition
                            return Some((self.file, range));
                        }
                    }
                }
            }
        }

        None
    }

    /// Resolves an imported symbol to its actual definition in the source module.
    fn resolve_imported_symbol_definition(
        &self,
        import_from: &crate::semantic_index::definition::ImportFromDefinitionKind
    ) -> Option<(File, TextRange)> {
        // Get the import statement and alias
        let import_stmt = import_from.import();
        let alias = import_from.alias();

        // Resolve the module name (handles both absolute and relative imports)
        let module_name = crate::module_name::ModuleName::from_import_statement(
            self.db,
            self.file,
            import_stmt
        ).ok()?;

        let module = self.resolve_module(&module_name)?;

        // Find the symbol in the target module
        let target_file = module.file()?;
        let target_index = semantic_index(self.db, target_file);
        let binding = target_index.binding_by_name(&alias.name)?;
        let range = binding.focus_range(self.db).range();
        Some((target_file, range))
    }

    /// Resolves an attribute access expression to its definition location.
    /// This handles both local and cross-module attribute access.
    /// For example: `a.b.c.ccc` -> resolves `ccc` in module `a.b.c`
    pub fn resolve_attribute_definition(
        &self,
        expr: ast::ExprRef<'_>
    ) -> Option<(File, TextRange)> {
        // Handle simple name expressions (local symbols)
        if let Some(name_expr) = expr.as_name_expr() {
            return self.resolve_name_definition(name_expr.id.as_str());
        }

        // Handle attribute access expressions
        let attr_expr = expr.as_attribute_expr()?;

        // Special handling for cls.attr and self.attr
        if let ast::Expr::Name(name) = attr_expr.value.as_ref() {
            if name.id == "cls" || name.id == "self" {
                // Find the containing class scope and look up the attribute there
                let attr_name = attr_expr.attr.id.as_str();
                let index = semantic_index(self.db, self.file);
                let file_scope = index.expression_scope_id(expr);

                // Walk up to find the class scope
                for (_scope_id, scope) in index.ancestor_scopes(file_scope) {
                    if let Some(class_def_node) = scope.node().as_class() {
                        // Get the type of the class
                        let class_type = class_def_node.inferred_type(&SemanticModel::new(self.db, self.file));

                        // Try to extract ClassLiteral from the type
                        if let Some(class_literal) = class_type.into_class_literal() {
                            // Use the new helper to find which class defines this member
                            if let Some((defining_class, defining_file)) = class_literal.find_member_defining_class(self.db, None, attr_name) {
                                // Get the definition range
                                let (def_literal, _) = defining_class.class_literal(self.db);
                                let def_scope = def_literal.body_scope(self.db);
                                let def_index = semantic_index(self.db, defining_file);
                                let symbol_table = def_index.symbol_table(def_scope.file_scope_id(self.db));

                                if let Some(symbol_id) = symbol_table.symbol_id_by_name(attr_name) {
                                    let use_def = def_index.use_def_map(def_scope.file_scope_id(self.db));
                                    // Get all definitions and find the first one
                                    let mut first_def: Option<TextRange> = None;
                                    for definition in use_def.all_definitions_for_symbol(self.db, symbol_id) {
                                        let range = definition.focus_range(self.db).range();
                                        if let Some(first_range) = first_def {
                                            if range.start() < first_range.start() {
                                                first_def = Some(range);
                                            }
                                        } else {
                                            first_def = Some(range);
                                        }
                                    }
                                    if let Some(range) = first_def {
                                        return Some((defining_file, range));
                                    }
                                }
                            }
                        }
                        break; // Found the class, no need to continue
                    }
                }
                return None;
            }
        }

        // Try to resolve the attribute as a cross-module reference
        if let Some((target_module, symbol_name)) = self.resolve_module_attribute(attr_expr) {
            // Look up the symbol in the target module
            let target_file = target_module.file()?;
            let target_index = semantic_index(self.db, target_file);
            let binding = target_index.binding_by_name(&symbol_name)?;
            let range = binding.focus_range(self.db).range();
            return Some((target_file, range));
        }

        // If not a cross-module reference, try to resolve as an instance attribute
        // (this would handle cases like obj.method, a.m1(), etc.)
        self.resolve_instance_attribute_definition(attr_expr)
    }

    /// Resolves an attribute expression to a (module, symbol_name) pair if it represents
    /// a cross-module reference like `a.b.c.symbol`
    fn resolve_module_attribute(
        &self,
        attr_expr: &ast::ExprAttribute
    ) -> Option<(Module, String)> {
        let symbol_name = attr_expr.attr.id.to_string();
        
        // Try to resolve the value part as a module
        let value_type = self.infer_expression_type(&attr_expr.value)?;
        
        // Check if the value resolves to a module
        if let Some(module_literal) = value_type.into_module_literal() {
            let module = module_literal.module(self.db);
            return Some((module, symbol_name));
        }
        
        None
    }

    /// Infer the type of an expression (simplified version for module resolution)
    fn infer_expression_type(&self, expr: &ast::Expr) -> Option<Type<'_>> {
        let expr_ref = ast::ExprRef::from(expr);
        let index = semantic_index(self.db, self.file);
        let file_scope = index.expression_scope_id(expr_ref);
        let scope = file_scope.to_scope_id(self.db, self.file);
        let expression_id = expr_ref.scoped_expression_id(self.db, scope);

        // Get the type from inference
        let inferred_types = infer_scope_types(self.db, scope);
        Some(inferred_types.expression_type(expression_id))
    }

    /// Resolves an instance attribute (e.g., `obj.method`) to its definition location.
    fn resolve_instance_attribute_definition(
        &self,
        attr_expr: &ast::ExprAttribute
    ) -> Option<(File, TextRange)> {
        let attr_name = attr_expr.attr.id.as_str();

        // Get the type of the value (the object we're accessing the attribute on)
        let value_type = self.infer_expression_type(&attr_expr.value)?;

        // Try to get the class from the type
        // For instances, we need to check if it's an instance type
        let class_literal = if let Some(instance) = value_type.into_nominal_instance() {
            // Get the class from the instance type
            // ClassType::class_literal returns (ClassLiteral, Option<Specialization>)
            let (literal, _) = instance.class().class_literal(self.db);
            Some(literal)
        } else {
            // For class objects themselves (e.g., calling class methods), get the class directly
            value_type.into_class_literal()
        }?;

        // Find which class in the MRO defines this member
        let (defining_class, defining_file) = class_literal.find_member_defining_class(self.db, None, attr_name)?;

        // Get the definition range
        let (def_literal, _) = defining_class.class_literal(self.db);
        let def_scope = def_literal.body_scope(self.db);
        let def_index = semantic_index(self.db, defining_file);
        let symbol_table = def_index.symbol_table(def_scope.file_scope_id(self.db));

        // First, try to find it in the class body
        if let Some(symbol_id) = symbol_table.symbol_id_by_name(attr_name) {
            let use_def = def_index.use_def_map(def_scope.file_scope_id(self.db));

            // Get all definitions and find the first one (earliest in source order)
            let mut first_def: Option<TextRange> = None;
            for definition in use_def.all_definitions_for_symbol(self.db, symbol_id) {
                let range = definition.focus_range(self.db).range();
                if let Some(first_range) = first_def {
                    if range.start() < first_range.start() {
                        first_def = Some(range);
                    }
                } else {
                    first_def = Some(range);
                }
            }

            if let Some(range) = first_def {
                return Some((defining_file, range));
            }
        }

        // If not found in class body, look for instance attribute assignments (e.g., self.x = 2)
        use crate::semantic_index::attribute_assignments;
        for (bindings, _method_scope_id) in attribute_assignments(self.db, def_scope, attr_name) {
            // Get the first binding (earliest in source order)
            let mut first_def: Option<TextRange> = None;
            for binding_with_constraints in bindings {
                if let Some(binding) = binding_with_constraints.binding {
                    let range = binding.focus_range(self.db).range();
                    if let Some(first_range) = first_def {
                        if range.start() < first_range.start() {
                            first_def = Some(range);
                        }
                    } else {
                        first_def = Some(range);
                    }
                }
            }

            if let Some(range) = first_def {
                return Some((defining_file, range));
            }
        }

        None
    }

    /// Returns completions for symbols available in the scope containing the
    /// given expression.
    ///
    /// If a scope could not be determined, then completions for the global
    /// scope of this model's `File` are returned.
    pub fn completions(&self, node: ast::AnyNodeRef<'_>) -> Vec<Name> {
        let index = semantic_index(self.db, self.file);
        let file_scope = match node {
            ast::AnyNodeRef::Identifier(identifier) => index.expression_scope_id(identifier),
            node => match node.as_expr_ref() {
                // If we couldn't identify a specific
                // expression that we're in, then just
                // fall back to the global scope.
                None => FileScopeId::global(),
                Some(expr) => index.expression_scope_id(expr),
            },
        };
        let mut symbols = vec![];
        for (file_scope, _) in index.ancestor_scopes(file_scope) {
            for symbol in index.symbol_table(file_scope).symbols() {
                symbols.push(symbol.name().clone());
            }
        }
        symbols
    }
}

pub trait HasType {
    /// Returns the inferred type of `self`.
    ///
    /// ## Panics
    /// May panic if `self` is from another file than `model`.
    fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db>;
}

impl HasType for ast::ExprRef<'_> {
    fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db> {
        let index = semantic_index(model.db, model.file);
        let file_scope = index.expression_scope_id(*self);
        let scope = file_scope.to_scope_id(model.db, model.file);

        let expression_id = self.scoped_expression_id(model.db, scope);
        infer_scope_types(model.db, scope).expression_type(expression_id)
    }
}

macro_rules! impl_expression_has_type {
    ($ty: ty) => {
        impl HasType for $ty {
            #[inline]
            fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db> {
                let expression_ref = ExprRef::from(self);
                expression_ref.inferred_type(model)
            }
        }
    };
}

impl_expression_has_type!(ast::ExprBoolOp);
impl_expression_has_type!(ast::ExprNamed);
impl_expression_has_type!(ast::ExprBinOp);
impl_expression_has_type!(ast::ExprUnaryOp);
impl_expression_has_type!(ast::ExprLambda);
impl_expression_has_type!(ast::ExprIf);
impl_expression_has_type!(ast::ExprDict);
impl_expression_has_type!(ast::ExprSet);
impl_expression_has_type!(ast::ExprListComp);
impl_expression_has_type!(ast::ExprSetComp);
impl_expression_has_type!(ast::ExprDictComp);
impl_expression_has_type!(ast::ExprGenerator);
impl_expression_has_type!(ast::ExprAwait);
impl_expression_has_type!(ast::ExprYield);
impl_expression_has_type!(ast::ExprYieldFrom);
impl_expression_has_type!(ast::ExprCompare);
impl_expression_has_type!(ast::ExprCall);
impl_expression_has_type!(ast::ExprFString);
impl_expression_has_type!(ast::ExprTString);
impl_expression_has_type!(ast::ExprStringLiteral);
impl_expression_has_type!(ast::ExprBytesLiteral);
impl_expression_has_type!(ast::ExprNumberLiteral);
impl_expression_has_type!(ast::ExprBooleanLiteral);
impl_expression_has_type!(ast::ExprNoneLiteral);
impl_expression_has_type!(ast::ExprEllipsisLiteral);
impl_expression_has_type!(ast::ExprAttribute);
impl_expression_has_type!(ast::ExprSubscript);
impl_expression_has_type!(ast::ExprStarred);
impl_expression_has_type!(ast::ExprName);
impl_expression_has_type!(ast::ExprList);
impl_expression_has_type!(ast::ExprTuple);
impl_expression_has_type!(ast::ExprSlice);
impl_expression_has_type!(ast::ExprIpyEscapeCommand);

impl HasType for ast::Expr {
    fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db> {
        match self {
            Expr::BoolOp(inner) => inner.inferred_type(model),
            Expr::Named(inner) => inner.inferred_type(model),
            Expr::BinOp(inner) => inner.inferred_type(model),
            Expr::UnaryOp(inner) => inner.inferred_type(model),
            Expr::Lambda(inner) => inner.inferred_type(model),
            Expr::If(inner) => inner.inferred_type(model),
            Expr::Dict(inner) => inner.inferred_type(model),
            Expr::Set(inner) => inner.inferred_type(model),
            Expr::ListComp(inner) => inner.inferred_type(model),
            Expr::SetComp(inner) => inner.inferred_type(model),
            Expr::DictComp(inner) => inner.inferred_type(model),
            Expr::Generator(inner) => inner.inferred_type(model),
            Expr::Await(inner) => inner.inferred_type(model),
            Expr::Yield(inner) => inner.inferred_type(model),
            Expr::YieldFrom(inner) => inner.inferred_type(model),
            Expr::Compare(inner) => inner.inferred_type(model),
            Expr::Call(inner) => inner.inferred_type(model),
            Expr::FString(inner) => inner.inferred_type(model),
            Expr::TString(inner) => inner.inferred_type(model),
            Expr::StringLiteral(inner) => inner.inferred_type(model),
            Expr::BytesLiteral(inner) => inner.inferred_type(model),
            Expr::NumberLiteral(inner) => inner.inferred_type(model),
            Expr::BooleanLiteral(inner) => inner.inferred_type(model),
            Expr::NoneLiteral(inner) => inner.inferred_type(model),
            Expr::EllipsisLiteral(inner) => inner.inferred_type(model),
            Expr::Attribute(inner) => inner.inferred_type(model),
            Expr::Subscript(inner) => inner.inferred_type(model),
            Expr::Starred(inner) => inner.inferred_type(model),
            Expr::Name(inner) => inner.inferred_type(model),
            Expr::List(inner) => inner.inferred_type(model),
            Expr::Tuple(inner) => inner.inferred_type(model),
            Expr::Slice(inner) => inner.inferred_type(model),
            Expr::IpyEscapeCommand(inner) => inner.inferred_type(model),
        }
    }
}

macro_rules! impl_binding_has_ty {
    ($ty: ty) => {
        impl HasType for $ty {
            #[inline]
            fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db> {
                let index = semantic_index(model.db, model.file);
                let binding = index.expect_single_definition(self);
                binding_type(model.db, binding)
            }
        }
    };
}

impl_binding_has_ty!(ast::StmtFunctionDef);
impl_binding_has_ty!(ast::StmtClassDef);
impl_binding_has_ty!(ast::Parameter);
impl_binding_has_ty!(ast::ParameterWithDefault);
impl_binding_has_ty!(ast::ExceptHandlerExceptHandler);

impl HasType for ast::Alias {
    fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Type<'db> {
        if &self.name == "*" {
            return Type::Never;
        }
        let index = semantic_index(model.db, model.file);
        binding_type(model.db, index.expect_single_definition(self))
    }
}

#[cfg(test)]
mod tests {
    use ruff_db::files::system_path_to_file;
    use ruff_db::parsed::parsed_module;

    use crate::db::tests::TestDbBuilder;
    use crate::{HasType, SemanticModel};

    #[test]
    fn function_type() -> anyhow::Result<()> {
        let db = TestDbBuilder::new()
            .with_file("/src/foo.py", "def test(): pass")
            .build()?;

        let foo = system_path_to_file(&db, "/src/foo.py").unwrap();

        let ast = parsed_module(&db, foo);

        let function = ast.suite()[0].as_function_def_stmt().unwrap();
        let model = SemanticModel::new(&db, foo);
        let ty = function.inferred_type(&model);

        assert!(ty.is_function_literal());

        Ok(())
    }

    #[test]
    fn class_type() -> anyhow::Result<()> {
        let db = TestDbBuilder::new()
            .with_file("/src/foo.py", "class Test: pass")
            .build()?;

        let foo = system_path_to_file(&db, "/src/foo.py").unwrap();

        let ast = parsed_module(&db, foo);

        let class = ast.suite()[0].as_class_def_stmt().unwrap();
        let model = SemanticModel::new(&db, foo);
        let ty = class.inferred_type(&model);

        assert!(ty.is_class_literal());

        Ok(())
    }

    #[test]
    fn alias_type() -> anyhow::Result<()> {
        let db = TestDbBuilder::new()
            .with_file("/src/foo.py", "class Test: pass")
            .with_file("/src/bar.py", "from foo import Test")
            .build()?;

        let bar = system_path_to_file(&db, "/src/bar.py").unwrap();

        let ast = parsed_module(&db, bar);

        let import = ast.suite()[0].as_import_from_stmt().unwrap();
        let alias = &import.names[0];
        let model = SemanticModel::new(&db, bar);
        let ty = alias.inferred_type(&model);

        assert!(ty.is_class_literal());

        Ok(())
    }
}
