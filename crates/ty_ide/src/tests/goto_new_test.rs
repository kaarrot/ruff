#[cfg(test)]
mod tests {
    use crate::tests::cursor_test;
    use insta::assert_snapshot;

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
         --> main.py:3:17
          |
        3 |                 test = 1
          |                 ^^^^
          |
        info: Source
         --> main.py:7:25
          |
        7 |                     cls.test = 2
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
}