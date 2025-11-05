"""
#[cfg(test)]
mod tests {
    use crate::tests::{CursorTest, IntoDiagnostic, cursor_test};
    use crate::{NavigationTarget, goto_type_definition, goto_definition};
    use insta::assert_snapshot;
    use ruff_db::diagnostic::{
        Annotation, Diagnostic, DiagnosticId, LintName, Severity, Span, SubDiagnostic,
    };
    use ruff_db::files::FileRange;
    use ruff_text_size::Ranged;
    use ruff_db::source::source_text;

    #[test]
    fn goto_class_method_access() {
        let test = cursor_test(
            r#"
            class A:
                test = 1

                @classmethod
                def m1(cls):
                    cls.te<CURSOR>st = 2
            "#,
        );

        assert_snapshot!(test.goto_definition(), @r#"
        info[goto-definition]: Definition
         --> main.py:2:17
          |
        2 |                 test = 1
          |                 ^^^^
          |
        info: Source
         --> main.py:6:25
          |
        6 |                     cls.test = 2
          |                         ^^^^
          |
        "#);
    }

    #[test]
    fn goto_local_variable() {
        let test = cursor_test(
            r#"
            def bbb():
                aaa = 1
                a<CURSOR>aa = aaa + 1
            "#,
        );

        assert_snapshot!(test.goto_definition(), @r#"
        info[goto-definition]: Definition
         --> main.py:3:17
          |
        3 |                 aaa = 1
          |                 ^^^
          |
        info: Source
         --> main.py:4:17
          |
        4 |                 aaa = aaa + 1
          |                 ^^^
          |
        "#);
    }
    #[test]
    fn goto_local_function_parameter() {
        let test = cursor_test(
            r#"""
            def foo(param1, param2):
                print(par<CURSOR>am1)
            """#,
        );

        assert_snapshot!(test.goto_definition(), @r#"""
        info[goto-definition]: Definition
         --> main.py:2:13
          |
        2 |             def foo(param1, param2):
          |                     ^^^^^^
          |
        info: Source
         --> main.py:3:17
          |
        3 |                 print(param1)
          |                       ^^^^^^
          |
        """#);
    }

}

""