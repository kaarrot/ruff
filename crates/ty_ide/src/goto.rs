use crate::find_node::covering_node;
use crate::{Db, HasNavigationTargets, NavigationTarget, NavigationTargets, RangedValue};
use ruff_db::files::{File, FileRange};
use ruff_db::parsed::{parsed_module, ParsedModule};
use ruff_python_ast::{self as ast, AnyNodeRef};
use ruff_python_parser::TokenKind;
use ruff_text_size::{Ranged, TextRange, TextSize};
use ty_python_semantic::types::Type;
use ty_python_semantic::{HasType, SemanticModel};

pub fn goto_type_definition(
    db: &dyn Db,
    file: File,
    offset: TextSize,
) -> Option<RangedValue<NavigationTargets>> {
    let parsed = parsed_module(db.upcast(), file);
    let goto_target = find_goto_target(&parsed, offset)?;

    let model = SemanticModel::new(db.upcast(), file);
    let ty = goto_target.inferred_type(&model)?;

    tracing::debug!(
        "Inferred type of covering node is {}",
        ty.display(db.upcast())
    );

    let navigation_targets = ty.navigation_targets(db);

    Some(RangedValue {
        range: FileRange::new(file, goto_target.range()),
        value: navigation_targets,
    })
}

pub fn goto_definition(
    db: &dyn Db,
    file: File,
    offset: TextSize,
) -> Option<RangedValue<NavigationTargets>> {
    let parsed = parsed_module(db.upcast(), file);
    let goto_target = find_goto_target(&parsed, offset)?;

    // Get the definition target, not the type
    let navigation_targets = goto_target.navigation_targets(db, file)?;

    Some(RangedValue {
        range: FileRange::new(file, goto_target.range()),
        value: navigation_targets,
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum GotoTarget<'a> {
    Expression(ast::ExprRef<'a>),
    FunctionDef(&'a ast::StmtFunctionDef),
    ClassDef(&'a ast::StmtClassDef),
    Parameter(&'a ast::Parameter),
    Alias(&'a ast::Alias),

    /// Go to on the module name of an import from
    /// ```py
    /// from foo import bar
    ///      ^^^
    /// ```
    ImportedModule(&'a ast::StmtImportFrom),

    /// Go to on the exception handler variable
    /// ```py
    /// try: ...
    /// except Exception as e: ...
    ///                     ^
    /// ```
    ExceptVariable(&'a ast::ExceptHandlerExceptHandler),

    /// Go to on a keyword argument
    /// ```py
    /// test(a = 1)
    ///      ^
    /// ```
    KeywordArgument(&'a ast::Keyword),

    /// Go to on the rest parameter of a pattern match
    ///
    /// ```py
    /// match x:
    ///     case {"a": a, "b": b, **rest}: ...
    ///                             ^^^^
    /// ```
    PatternMatchRest(&'a ast::PatternMatchMapping),

    /// Go to on a keyword argument of a class pattern
    ///
    /// ```py
    /// match Point3D(0, 0, 0):
    ///     case Point3D(x=0, y=0, z=0): ...
    ///                  ^    ^    ^
    /// ```
    PatternKeywordArgument(&'a ast::PatternKeyword),

    /// Go to on a pattern star argument
    ///
    /// ```py
    /// match array:
    ///     case [*args]: ...
    ///            ^^^^
    PatternMatchStarName(&'a ast::PatternMatchStar),

    /// Go to on the name of a pattern match as pattern
    ///
    /// ```py
    /// match x:
    ///     case [x] as y: ...
    ///                 ^
    PatternMatchAsName(&'a ast::PatternMatchAs),

    /// Go to on the name of a type variable
    ///
    /// ```py
    /// type Alias[T: int = bool] = list[T]
    ///            ^
    /// ```
    TypeParamTypeVarName(&'a ast::TypeParamTypeVar),

    /// Go to on the name of a type param spec
    ///
    /// ```py
    /// type Alias[**P = [int, str]] = Callable[P, int]
    ///              ^
    /// ```
    TypeParamParamSpecName(&'a ast::TypeParamParamSpec),

    /// Go to on the name of a type var tuple
    ///
    /// ```py
    /// type Alias[*Ts = ()] = tuple[*Ts]
    ///             ^^
    /// ```
    TypeParamTypeVarTupleName(&'a ast::TypeParamTypeVarTuple),

    NonLocal {
        identifier: &'a ast::Identifier,
    },
    Globals {
        identifier: &'a ast::Identifier,
    },
}

impl<'db> GotoTarget<'db> {
    pub(crate) fn inferred_type(self, model: &SemanticModel<'db>) -> Option<Type<'db>> {
        let ty = match self {
            GotoTarget::Expression(expression) => expression.inferred_type(model),
            GotoTarget::FunctionDef(function) => function.inferred_type(model),
            GotoTarget::ClassDef(class) => class.inferred_type(model),
            GotoTarget::Parameter(parameter) => parameter.inferred_type(model),
            GotoTarget::Alias(alias) => alias.inferred_type(model),
            GotoTarget::ExceptVariable(except) => except.inferred_type(model),
            GotoTarget::KeywordArgument(argument) => {
                // TODO: Pyright resolves the declared type of the matching parameter. This seems more accurate
                // than using the inferred value.
                argument.value.inferred_type(model)
            }
            // TODO: Support identifier targets
            GotoTarget::PatternMatchRest(_)
            | GotoTarget::PatternKeywordArgument(_)
            | GotoTarget::PatternMatchStarName(_)
            | GotoTarget::PatternMatchAsName(_)
            | GotoTarget::ImportedModule(_)
            | GotoTarget::TypeParamTypeVarName(_)
            | GotoTarget::TypeParamParamSpecName(_)
            | GotoTarget::TypeParamTypeVarTupleName(_)
            | GotoTarget::NonLocal { .. }
            | GotoTarget::Globals { .. } => return None,
        };

        Some(ty)
    }

    pub(crate) fn navigation_targets(self, db: &dyn Db, current_file: File) -> Option<NavigationTargets> {
        let model = SemanticModel::new(db.upcast(), current_file);
        match self {
            GotoTarget::Expression(expr) => {
                // For expressions, try to go to the name definition first
                if let Some(name_expr) = expr.as_name_expr() {
                    if let Some((file, range)) = model.resolve_name_definition(name_expr.id.as_str()) {
                        return Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }));
                    }
                }
                // If not a name expression or no definition found, this is not a valid goto definition target
                None
            }
            GotoTarget::FunctionDef(function) => {
                let name = function.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::ClassDef(class) => {
                let name = class.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::Parameter(parameter) => {
                let name = parameter.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::Alias(alias) => {
                let name = alias.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::ExceptVariable(except) => {
                if let Some(name) = except.name.as_ref() {
                    let name_str = name.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name_str) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            GotoTarget::KeywordArgument(argument) => {
                if let Some(arg) = argument.arg.as_ref() {
                    let name = arg.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            GotoTarget::PatternMatchRest(rest) => {
                if let Some(name) = rest.rest.as_ref() {
                    let name_str = name.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name_str) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            GotoTarget::PatternKeywordArgument(keyword) => {
                let name = keyword.attr.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::PatternMatchStarName(star) => {
                if let Some(name) = star.name.as_ref() {
                    let name_str = name.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name_str) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            GotoTarget::PatternMatchAsName(as_pattern) => {
                if let Some(name) = as_pattern.name.as_ref() {
                    let name_str = name.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name_str) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            GotoTarget::TypeParamTypeVarName(var) => {
                let name = var.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::TypeParamParamSpecName(bound) => {
                let name = bound.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::TypeParamTypeVarTupleName(tuple) => {
                let name = tuple.name.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::NonLocal { identifier } => {
                let name = identifier.id.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::Globals { identifier } => {
                let name = identifier.id.as_str();
                if let Some((file, range)) = model.resolve_name_definition(name) {
                    Some(NavigationTargets::single(NavigationTarget {
                        file,
                        focus_range: range,
                        full_range: range,
                    }))
                } else {
                    None
                }
            }
            GotoTarget::ImportedModule(module) => {
                if let Some(module_name) = module.module.as_ref() {
                    let name = module_name.id.as_str();
                    if let Some((file, range)) = model.resolve_name_definition(name) {
                        Some(NavigationTargets::single(NavigationTarget {
                            file,
                            focus_range: range,
                            full_range: range,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
}

impl Ranged for GotoTarget<'_> {
    fn range(&self) -> TextRange {
        match self {
            GotoTarget::Expression(expression) => expression.range(),
            GotoTarget::FunctionDef(function) => function.name.range,
            GotoTarget::ClassDef(class) => class.name.range,
            GotoTarget::Parameter(parameter) => parameter.name.range,
            GotoTarget::Alias(alias) => alias.name.range,
            GotoTarget::ImportedModule(module) => module.module.as_ref().unwrap().range,
            GotoTarget::ExceptVariable(except) => except.name.as_ref().unwrap().range,
            GotoTarget::KeywordArgument(keyword) => keyword.arg.as_ref().unwrap().range,
            GotoTarget::PatternMatchRest(rest) => rest.rest.as_ref().unwrap().range,
            GotoTarget::PatternKeywordArgument(keyword) => keyword.attr.range,
            GotoTarget::PatternMatchStarName(star) => star.name.as_ref().unwrap().range,
            GotoTarget::PatternMatchAsName(as_pattern) => as_pattern.name.as_ref().unwrap().range,
            GotoTarget::TypeParamTypeVarName(type_var) => type_var.name.range,
            GotoTarget::TypeParamParamSpecName(spec) => spec.name.range,
            GotoTarget::TypeParamTypeVarTupleName(tuple) => tuple.name.range,
            GotoTarget::NonLocal { identifier, .. } => identifier.range,
            GotoTarget::Globals { identifier, .. } => identifier.range,
        }
    }
}

pub(crate) fn find_goto_target(parsed: &ParsedModule, offset: TextSize) -> Option<GotoTarget> {
    let token = parsed
        .tokens()
        .at_offset(offset)
        .max_by_key(|token| match token.kind() {
            TokenKind::Name
            | TokenKind::String
            | TokenKind::Complex
            | TokenKind::Float
            | TokenKind::Int => 1,
            _ => 0,
        })?;

    let covering_node = covering_node(parsed.syntax().into(), token.range())
        .find(|node| node.is_identifier() || node.is_expression())
        .ok()?;

    tracing::trace!("Covering node is of kind {:?}", covering_node.node().kind());

    match covering_node.node() {
        AnyNodeRef::Identifier(identifier) => match covering_node.parent() {
            Some(AnyNodeRef::StmtFunctionDef(function)) => Some(GotoTarget::FunctionDef(function)),
            Some(AnyNodeRef::StmtClassDef(class)) => Some(GotoTarget::ClassDef(class)),
            Some(AnyNodeRef::Parameter(parameter)) => Some(GotoTarget::Parameter(parameter)),
            Some(AnyNodeRef::Alias(alias)) => Some(GotoTarget::Alias(alias)),
            Some(AnyNodeRef::StmtImportFrom(from)) => Some(GotoTarget::ImportedModule(from)),
            Some(AnyNodeRef::ExceptHandlerExceptHandler(handler)) => {
                Some(GotoTarget::ExceptVariable(handler))
            }
            Some(AnyNodeRef::Keyword(keyword)) => Some(GotoTarget::KeywordArgument(keyword)),
            Some(AnyNodeRef::PatternMatchMapping(mapping)) => {
                Some(GotoTarget::PatternMatchRest(mapping))
            }
            Some(AnyNodeRef::PatternKeyword(keyword)) => {
                Some(GotoTarget::PatternKeywordArgument(keyword))
            }
            Some(AnyNodeRef::PatternMatchStar(star)) => {
                Some(GotoTarget::PatternMatchStarName(star))
            }
            Some(AnyNodeRef::PatternMatchAs(as_pattern)) => {
                Some(GotoTarget::PatternMatchAsName(as_pattern))
            }
            Some(AnyNodeRef::TypeParamTypeVar(var)) => Some(GotoTarget::TypeParamTypeVarName(var)),
            Some(AnyNodeRef::TypeParamParamSpec(bound)) => {
                Some(GotoTarget::TypeParamParamSpecName(bound))
            }
            Some(AnyNodeRef::TypeParamTypeVarTuple(var_tuple)) => {
                Some(GotoTarget::TypeParamTypeVarTupleName(var_tuple))
            }
            Some(AnyNodeRef::ExprAttribute(attribute)) => {
                Some(GotoTarget::Expression(attribute.into()))
            }
            Some(AnyNodeRef::StmtNonlocal(_)) => Some(GotoTarget::NonLocal { identifier }),
            Some(AnyNodeRef::StmtGlobal(_)) => Some(GotoTarget::Globals { identifier }),
            None => None,
            Some(parent) => {
                tracing::debug!(
                    "Missing `GoToTarget` for identifier with parent {:?}",
                    parent.kind()
                );
                None
            }
        },

        node => node.as_expr_ref().map(GotoTarget::Expression),
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{cursor_test, CursorTest, IntoDiagnostic};
    use crate::{goto_type_definition, goto_definition, NavigationTarget};
    use insta::assert_snapshot;
    use ruff_db::diagnostic::{
        Annotation, Diagnostic, DiagnosticId, LintName, Severity, Span, SubDiagnostic,
    };
    use ruff_db::files::FileRange;
    use ruff_text_size::Ranged;

    #[test]
    fn goto_type_of_expression_with_class_type() {
        let test = cursor_test(
            r#"
            class Test: ...

            a<CURSOR>b = Test()
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:2:19
          |
        2 |             class Test: ...
          |                   ^^^^
        3 |
        4 |             ab = Test()
          |
        info: Source
         --> main.py:4:13
          |
        2 |             class Test: ...
        3 |
        4 |             ab = Test()
          |             ^^
          |
        ");
    }

    #[test]
    fn goto_type_of_expression_with_function_type() {
        let test = cursor_test(
            r#"
            def foo(a, b): ...

            ab = foo

            a<CURSOR>b
        "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:2:17
          |
        2 |             def foo(a, b): ...
          |                 ^^^
        3 |
        4 |             ab = foo
          |
        info: Source
         --> main.py:6:13
          |
        4 |             ab = foo
        5 |
        6 |             ab
          |             ^^
          |
        ");
    }

    #[test]
    fn goto_type_of_expression_with_union_type() {
        let test = cursor_test(
            r#"

            def foo(a, b): ...

            def bar(a, b): ...

            if random.choice():
                a = foo
            else:
                a = bar

            a<CURSOR>
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:3:17
          |
        3 |             def foo(a, b): ...
          |                 ^^^
        4 |
        5 |             def bar(a, b): ...
          |
        info: Source
          --> main.py:12:13
           |
        10 |                 a = bar
        11 |
        12 |             a
           |             ^
           |

        info[goto-type-definition]: Type definition
         --> main.py:5:17
          |
        3 |             def foo(a, b): ...
        4 |
        5 |             def bar(a, b): ...
          |                 ^^^
        6 |
        7 |             if random.choice():
          |
        info: Source
          --> main.py:12:13
           |
        10 |                 a = bar
        11 |
        12 |             a
           |             ^
           |
        ");
    }

    #[test]
    fn goto_type_of_expression_with_module() {
        let mut test = cursor_test(
            r#"
            import lib

            lib<CURSOR>
            "#,
        );

        test.write_file("lib.py", "a = 10").unwrap();

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> lib.py:1:1
          |
        1 | a = 10
          | ^^^^^^
          |
        info: Source
         --> main.py:4:13
          |
        2 |             import lib
        3 |
        4 |             lib
          |             ^^^
          |
        ");
    }

    #[test]
    fn goto_type_of_expression_with_literal_type() {
        let test = cursor_test(
            r#"
            a: str = "test"

            a<CURSOR>
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r#"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:4:13
          |
        2 |             a: str = "test"
        3 |
        4 |             a
          |             ^
          |
        "#);
    }
    #[test]
    fn goto_type_of_expression_with_literal_node() {
        let test = cursor_test(
            r#"
            a: str = "te<CURSOR>st"
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r#"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:2:22
          |
        2 |             a: str = "test"
          |                      ^^^^^^
          |
        "#);
    }

    #[test]
    fn goto_type_of_expression_with_type_var_type() {
        let test = cursor_test(
            r#"
            type Alias[T: int = bool] = list[T<CURSOR>]
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:2:24
          |
        2 |             type Alias[T: int = bool] = list[T]
          |                        ^
          |
        info: Source
         --> main.py:2:46
          |
        2 |             type Alias[T: int = bool] = list[T]
          |                                              ^
          |
        ");
    }

    #[test]
    fn goto_type_of_expression_with_type_param_spec() {
        let test = cursor_test(
            r#"
            type Alias[**P = [int, str]] = Callable[P<CURSOR>, int]
            "#,
        );

        // TODO: Goto type definition currently doesn't work for type param specs
        // because the inference doesn't support them yet.
        // This snapshot should show a single target pointing to `T`
        assert_snapshot!(test.goto_type_definition(), @"No type definitions found");
    }

    #[test]
    fn goto_type_of_expression_with_type_var_tuple() {
        let test = cursor_test(
            r#"
            type Alias[*Ts = ()] = tuple[*Ts<CURSOR>]
            "#,
        );

        // TODO: Goto type definition currently doesn't work for type var tuples
        // because the inference doesn't support them yet.
        // This snapshot should show a single target pointing to `T`
        assert_snapshot!(test.goto_type_definition(), @"No type definitions found");
    }

    #[test]
    fn goto_type_on_keyword_argument() {
        let test = cursor_test(
            r#"
            def test(a: str): ...

            test(a<CURSOR>= "123")
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r#"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:4:18
          |
        2 |             def test(a: str): ...
        3 |
        4 |             test(a= "123")
          |                  ^
          |
        "#);
    }

    #[test]
    fn goto_type_on_incorrectly_typed_keyword_argument() {
        let test = cursor_test(
            r#"
            def test(a: str): ...

            test(a<CURSOR>= 123)
            "#,
        );

        // TODO: This should jump to `str` and not `int` because
        //   the keyword is typed as a string. It's only the passed argument that
        //   is an int. Navigating to `str` would match pyright's behavior.
        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:231:7
            |
        229 | _LiteralInteger = _PositiveInteger | _NegativeInteger | Literal[0]  # noqa: Y026  # TODO: Use TypeAlias once mypy bugs are fixed
        230 |
        231 | class int:
            |       ^^^
        232 |     @overload
        233 |     def __new__(cls, x: ConvertibleToInt = ..., /) -> Self: ...
            |
        info: Source
         --> main.py:4:18
          |
        2 |             def test(a: str): ...
        3 |
        4 |             test(a= 123)
          |                  ^
          |
        ");
    }

    #[test]
    fn goto_type_on_kwargs() {
        let test = cursor_test(
            r#"
            def f(name: str): ...

kwargs = { "name": "test"}

f(**kwargs<CURSOR>)
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r#"
        info[goto-type-definition]: Type definition
            --> stdlib/builtins.pyi:1086:7
             |
        1084 |     def __class_getitem__(cls, item: Any, /) -> GenericAlias: ...
        1085 |
        1086 | class dict(MutableMapping[_KT, _VT]):
             |       ^^^^
        1087 |     # __init__ should be kept roughly in line with `collections.UserDict.__init__`, which has similar semantics
        1088 |     # Also multiprocessing.managers.SyncManager.dict()
             |
        info: Source
         --> main.py:6:5
          |
        4 | kwargs = { "name": "test"}
        5 |
        6 | f(**kwargs)
          |     ^^^^^^
          |
        "#);
    }

    #[test]
    fn goto_type_of_expression_with_builtin() {
        let test = cursor_test(
            r#"
            def foo(a: str):
                a<CURSOR>
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:3:17
          |
        2 |             def foo(a: str):
        3 |                 a
          |                 ^
          |
        ");
    }

    #[test]
    fn goto_type_definition_cursor_between_object_and_attribute() {
        let test = cursor_test(
            r#"
            class X:
                def foo(a, b): ...

            x = X()

            x<CURSOR>.foo()
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:2:19
          |
        2 |             class X:
          |                   ^
        3 |                 def foo(a, b): ...
          |
        info: Source
         --> main.py:7:13
          |
        5 |             x = X()
        6 |
        7 |             x.foo()
          |             ^
          |
        ");
    }

    #[test]
    fn goto_between_call_arguments() {
        let test = cursor_test(
            r#"
            def foo(a, b): ...

            foo<CURSOR>()
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
         --> main.py:2:17
          |
        2 |             def foo(a, b): ...
          |                 ^^^
        3 |
        4 |             foo()
          |
        info: Source
         --> main.py:4:13
          |
        2 |             def foo(a, b): ...
        3 |
        4 |             foo()
          |             ^^^
          |
        ");
    }

    #[test]
    fn goto_type_narrowing() {
        let test = cursor_test(
            r#"
            def foo(a: str | None, b):
                if a is not None:
                    print(a<CURSOR>)
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:4:27
          |
        2 |             def foo(a: str | None, b):
        3 |                 if a is not None:
        4 |                     print(a)
          |                           ^
          |
        ");
    }

    #[test]
    fn goto_type_none() {
        let test = cursor_test(
            r#"
            def foo(a: str | None, b):
                a<CURSOR>
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r"
        info[goto-type-definition]: Type definition
           --> stdlib/types.pyi:671:11
            |
        669 | if sys.version_info >= (3, 10):
        670 |     @final
        671 |     class NoneType:
            |           ^^^^^^^^
        672 |         def __bool__(self) -> Literal[False]: ...
            |
        info: Source
         --> main.py:3:17
          |
        2 |             def foo(a: str | None, b):
        3 |                 a
          |                 ^
          |

        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:3:17
          |
        2 |             def foo(a: str | None, b):
        3 |                 a
          |                 ^
          |
        ");
    }

    #[test]
    fn goto_expression_with_utf8_encoding() {
        let test = cursor_test(
            r#"
            x = "test"
            x<CURSOR>
            "#,
        );

        assert_snapshot!(test.goto_type_definition(), @r###"
        info[goto-type-definition]: Type definition
           --> stdlib/builtins.pyi:438:7
            |
        436 |     def __getitem__(self, key: int, /) -> str | int | None: ...
        437 |
        438 | class str(Sequence[str]):
            |       ^^^
        439 |     @overload
        440 |     def __new__(cls, object: object = ...) -> Self: ...
            |
        info: Source
         --> main.py:3:13
          |
        2 |             x = "test"
        3 |             x
          |             ^
          |
        "###);
    }

    #[test]
    fn goto_definition_function() {
        let test = cursor_test(
            r#"
            def foo():
                pass
            
            fo<CURSOR>o()
            "#,
        );

        // This should go to the function definition, not the type
        if let Some(targets) = goto_definition(&test.db, test.file, test.cursor_offset) {
            assert!(!targets.is_empty());
            // Should find the function definition
            let target = &targets.value.into_iter().next().unwrap();
            // The target should point to the function name in the definition
            assert!(target.focus_range().start().to_u32() > 0);
        } else {
            panic!("Expected to find goto definition target");
        }
    }

    impl CursorTest {
        fn goto_type_definition(&self) -> String {
            let Some(targets) = goto_type_definition(&self.db, self.file, self.cursor_offset)
            else {
                return "No goto target found".to_string();
            };

            if targets.is_empty() {
                return "No type definitions found".to_string();
            }

            let source = targets.range;
            self.render_diagnostics(
                targets
                    .into_iter()
                    .map(|target| GotoTypeDefinitionDiagnostic::new(source, &target)),
            )
        }
    }

    struct GotoTypeDefinitionDiagnostic {
        source: FileRange,
        target: FileRange,
    }

    impl GotoTypeDefinitionDiagnostic {
        fn new(source: FileRange, target: &NavigationTarget) -> Self {
            Self {
                source,
                target: FileRange::new(target.file(), target.focus_range()),
            }
        }
    }

    impl IntoDiagnostic for GotoTypeDefinitionDiagnostic {
        fn into_diagnostic(self) -> Diagnostic {
            let mut source = SubDiagnostic::new(Severity::Info, "Source");
            source.annotate(Annotation::primary(
                Span::from(self.source.file()).with_range(self.source.range()),
            ));

            let mut main = Diagnostic::new(
                DiagnosticId::Lint(LintName::of("goto-type-definition")),
                Severity::Info,
                "Type definition".to_string(),
            );
            main.annotate(Annotation::primary(
                Span::from(self.target.file()).with_range(self.target.range()),
            ));
            main.sub(source);

            main
        }
    }
}
