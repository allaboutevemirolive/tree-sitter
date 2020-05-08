use super::helpers::allocations;
use super::helpers::fixtures::get_language;
use std::fmt::Write;
use tree_sitter::{
    Language, Node, Parser, Query, QueryCapture, QueryCursor, QueryError, QueryMatch,
    QueryPredicate, QueryPredicateArg, QueryProperty,
};

#[test]
fn test_query_errors_on_invalid_syntax() {
    allocations::record(|| {
        let language = get_language("javascript");

        assert!(Query::new(language, "(if_statement)").is_ok());
        assert!(Query::new(language, "(if_statement condition:(identifier))").is_ok());

        // Mismatched parens
        assert_eq!(
            Query::new(language, "(if_statement"),
            Err(QueryError::Syntax(
                1,
                [
                    "(if_statement", //
                    "             ^",
                ]
                .join("\n")
            ))
        );
        assert_eq!(
            Query::new(language, "; comment 1\n; comment 2\n  (if_statement))"),
            Err(QueryError::Syntax(
                3,
                [
                    "  (if_statement))", //
                    "                ^",
                ]
                .join("\n")
            ))
        );

        // Return an error at the *beginning* of a bare identifier not followed a colon.
        // If there's a colon but no pattern, return an error at the end of the colon.
        assert_eq!(
            Query::new(language, "(if_statement identifier)"),
            Err(QueryError::Syntax(
                1,
                [
                    "(if_statement identifier)", //
                    "              ^",
                ]
                .join("\n")
            ))
        );
        assert_eq!(
            Query::new(language, "(if_statement condition:)"),
            Err(QueryError::Syntax(
                1,
                [
                    "(if_statement condition:)", //
                    "                        ^",
                ]
                .join("\n")
            ))
        );

        // Return an error at the beginning of an unterminated string.
        assert_eq!(
            Query::new(language, r#"(identifier) "h "#),
            Err(QueryError::Syntax(
                1,
                [
                    r#"(identifier) "h "#, //
                    r#"             ^"#,
                ]
                .join("\n")
            ))
        );

        assert_eq!(
            Query::new(language, r#"((identifier) ()"#),
            Err(QueryError::Syntax(
                1,
                [
                    "((identifier) ()", //
                    "                ^",
                ]
                .join("\n")
            ))
        );
        assert_eq!(
            Query::new(language, r#"((identifier) @x (eq? @x a"#),
            Err(QueryError::Syntax(
                1,
                [
                    r#"((identifier) @x (eq? @x a"#,
                    r#"                          ^"#,
                ]
                .join("\n")
            ))
        );
    });
}

#[test]
fn test_query_errors_on_invalid_symbols() {
    allocations::record(|| {
        let language = get_language("javascript");

        assert_eq!(
            Query::new(language, "(clas)"),
            Err(QueryError::NodeType(1, "clas".to_string()))
        );
        assert_eq!(
            Query::new(language, "(if_statement (arrayyyyy))"),
            Err(QueryError::NodeType(1, "arrayyyyy".to_string()))
        );
        assert_eq!(
            Query::new(language, "(if_statement condition: (non_existent3))"),
            Err(QueryError::NodeType(1, "non_existent3".to_string()))
        );
        assert_eq!(
            Query::new(language, "(if_statement condit: (identifier))"),
            Err(QueryError::Field(1, "condit".to_string()))
        );
        assert_eq!(
            Query::new(language, "(if_statement conditioning: (identifier))"),
            Err(QueryError::Field(1, "conditioning".to_string()))
        );
    });
}

#[test]
fn test_query_errors_on_invalid_conditions() {
    allocations::record(|| {
        let language = get_language("javascript");

        assert_eq!(
            Query::new(language, "((identifier) @id (@id))"),
            Err(QueryError::Predicate(
                "Expected predicate to start with a function name. Got @id.".to_string()
            ))
        );
        assert_eq!(
            Query::new(language, "((identifier) @id (eq? @id))"),
            Err(QueryError::Predicate(
                "Wrong number of arguments to eq? predicate. Expected 2, got 1.".to_string()
            ))
        );
        assert_eq!(
            Query::new(language, "((identifier) @id (eq? @id @ok))"),
            Err(QueryError::Capture(1, "ok".to_string()))
        );
    });
}

#[test]
fn test_query_matches_with_simple_pattern() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "(function_declaration name: (identifier) @fn-name)",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "function one() { two(); function three() {} }",
            &[
                (0, vec![("fn-name", "one")]),
                (0, vec![("fn-name", "three")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_multiple_on_same_root() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "(class_declaration
                name: (identifier) @the-class-name
                (class_body
                    (method_definition
                        name: (property_identifier) @the-method-name)))",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            class Person {
                // the constructor
                constructor(name) { this.name = name; }

                // the getter
                getFullName() { return this.name; }
            }
            ",
            &[
                (
                    0,
                    vec![
                        ("the-class-name", "Person"),
                        ("the-method-name", "constructor"),
                    ],
                ),
                (
                    0,
                    vec![
                        ("the-class-name", "Person"),
                        ("the-method-name", "getFullName"),
                    ],
                ),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_multiple_patterns_different_roots() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
                (function_declaration name:(identifier) @fn-def)
                (call_expression function:(identifier) @fn-ref)
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            function f1() {
                f2(f3());
            }
            ",
            &[
                (0, vec![("fn-def", "f1")]),
                (1, vec![("fn-ref", "f2")]),
                (1, vec![("fn-ref", "f3")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_multiple_patterns_same_root() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
              (pair
                key: (property_identifier) @method-def
                value: (function))

              (pair
                key: (property_identifier) @method-def
                value: (arrow_function))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            a = {
                b: () => { return c; },
                d: function() { return d; }
            };
            ",
            &[
                (1, vec![("method-def", "b")]),
                (0, vec![("method-def", "d")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_nesting_and_no_fields() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
                (array
                    (array
                        (identifier) @x1
                        (identifier) @x2))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            [[a]];
            [[c, d], [e, f, g, h]];
            [[h], [i]];
            ",
            &[
                (0, vec![("x1", "c"), ("x2", "d")]),
                (0, vec![("x1", "e"), ("x2", "f")]),
                (0, vec![("x1", "e"), ("x2", "g")]),
                (0, vec![("x1", "f"), ("x2", "g")]),
                (0, vec![("x1", "e"), ("x2", "h")]),
                (0, vec![("x1", "f"), ("x2", "h")]),
                (0, vec![("x1", "g"), ("x2", "h")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_many() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(language, "(array (identifier) @element)").unwrap();

        assert_query_matches(
            language,
            &query,
            &"[hello];\n".repeat(50),
            &vec![(0, vec![("element", "hello")]); 50],
        );
    });
}

#[test]
fn test_query_matches_capturing_error_nodes() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
            (ERROR (identifier) @the-error-identifier) @the-error
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "function a(b,, c, d :e:) {}",
            &[(0, vec![("the-error", ":e:"), ("the-error-identifier", "e")])],
        );
    });
}

#[test]
fn test_query_matches_with_named_wildcard() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
            (return_statement (*) @the-return-value)
            (binary_expression operator: * @the-operator)
            ",
        )
        .unwrap();

        let source = "return a + b - c;";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));

        assert_eq!(
            collect_matches(matches, &query, source),
            &[
                (0, vec![("the-return-value", "a + b - c")]),
                (1, vec![("the-operator", "+")]),
                (1, vec![("the-operator", "-")]),
            ]
        );
    });
}

#[test]
fn test_query_matches_with_wildcard_at_the_root() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
            (*
                (comment) @doc
                .
                (function_declaration
                    name: (identifier) @name))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "/* one */ var x; /* two */ function y() {} /* three */ class Z {}",
            &[(0, vec![("doc", "/* two */"), ("name", "y")])],
        );

        let query = Query::new(
            language,
            "
                (* (string) @a)
                (* (number) @b)
                (* (true) @c)
                (* (false) @d)
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "['hi', x(true), {y: false}]",
            &[
                (0, vec![("a", "'hi'")]),
                (2, vec![("c", "true")]),
                (3, vec![("d", "false")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_immediate_siblings() {
    allocations::record(|| {
        let language = get_language("python");

        // The immediate child operator '.' can be used in three similar ways:
        // 1. Before the first child node in a pattern, it means that there cannot be any
        //    named siblings before that child node.
        // 2. After the last child node in a pattern, it means that there cannot be any named
        //    sibling after that child node.
        // 2. Between two child nodes in a pattern, it specifies that there cannot be any
        //    named siblings between those two child snodes.
        let query = Query::new(
            language,
            "
            (dotted_name
                (identifier) @parent
                .
                (identifier) @child)
            (dotted_name
                (identifier) @last-child
                .)
            (list
                .
                (*) @first-element)
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "import a.b.c.d; return [w, [1, y], z]",
            &[
                (0, vec![("parent", "a"), ("child", "b")]),
                (0, vec![("parent", "b"), ("child", "c")]),
                (1, vec![("last-child", "d")]),
                (0, vec![("parent", "c"), ("child", "d")]),
                (2, vec![("first-element", "w")]),
                (2, vec![("first-element", "1")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_repeated_leaf_nodes() {
    allocations::record(|| {
        let language = get_language("javascript");

        let query = Query::new(
            language,
            "
            (*
                (comment)+ @doc
                .
                (class_declaration
                    name: (identifier) @name))

            (*
                (comment)+ @doc
                .
                (function_declaration
                    name: (identifier) @name))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            // one
            // two
            a();

            // three
            {
                // four
                // five
                // six
                class B {}

                // seven
                c();

                // eight
                function d() {}
            }
            ",
            &[
                (
                    0,
                    vec![
                        ("doc", "// four"),
                        ("doc", "// five"),
                        ("doc", "// six"),
                        ("name", "B"),
                    ],
                ),
                (1, vec![("doc", "// eight"), ("name", "d")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_optional_nodes_inside_of_repetitions() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(language, r#"(array (","? (number) @num)+)"#).unwrap();

        assert_query_matches(
            language,
            &query,
            r#"
            var a = [1, 2, 3, 4]
            "#,
            &[(
                0,
                vec![("num", "1"), ("num", "2"), ("num", "3"), ("num", "4")],
            )],
        );
    });
}

#[test]
fn test_query_matches_with_top_level_repetitions() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (comment)+ @doc
            "#,
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            r#"
            // a
            // b
            // c

            d()

            // e
            "#,
            &[
                (0, vec![("doc", "// a"), ("doc", "// b"), ("doc", "// c")]),
                (0, vec![("doc", "// e")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_nested_repetitions() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (variable_declaration
                (","? (variable_declarator name: (identifier) @x))+)+
            "#,
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            r#"
            var a = b, c, d
            var e, f

            // more
            var g
            "#,
            &[
                (
                    0,
                    vec![("x", "a"), ("x", "c"), ("x", "d"), ("x", "e"), ("x", "f")],
                ),
                (0, vec![("x", "g")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_leading_optional_repeated_leaf_nodes() {
    allocations::record(|| {
        let language = get_language("javascript");

        let query = Query::new(
            language,
            "
            (*
                (comment)+? @doc
                .
                (function_declaration
                    name: (identifier) @name))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            function a() {
                // one
                var b;

                function c() {}

                // two
                // three
                var d;

                // four
                // five
                function e() {

                }
            }

            // six
            ",
            &[
                (0, vec![("name", "a")]),
                (0, vec![("name", "c")]),
                (
                    0,
                    vec![("doc", "// four"), ("doc", "// five"), ("name", "e")],
                ),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_trailing_optional_nodes() {
    allocations::record(|| {
        let language = get_language("javascript");

        let query = Query::new(
            language,
            "
            (class_declaration
                name: (identifier) @class
                (class_heritage
                  (identifier) @superclass)?)
            ",
        )
        .unwrap();

        assert_query_matches(language, &query, "class A {}", &[(0, vec![("class", "A")])]);

        assert_query_matches(
            language,
            &query,
            "
            class A {}
            class B extends C {}
            class D extends (E.F) {}
            ",
            &[
                (0, vec![("class", "A")]),
                (0, vec![("class", "B"), ("superclass", "C")]),
                (0, vec![("class", "D")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_repeated_internal_nodes() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
            (*
                (method_definition
                    (decorator (identifier) @deco)+
                    name: (property_identifier) @name))
            ",
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "
            class A {
                @c
                @d
                e() {}
            }
            ",
            &[(0, vec![("deco", "c"), ("deco", "d"), ("name", "e")])],
        );
    })
}

#[test]
fn test_query_matches_in_language_with_simple_aliases() {
    allocations::record(|| {
        let language = get_language("html");

        // HTML uses different tokens to track start tags names, end
        // tag names, script tag names, and style tag names. All of
        // these tokens are aliased to `tag_name`.
        let query = Query::new(language, "(tag_name) @tag").unwrap();

        assert_query_matches(
            language,
            &query,
            "
            <div>
                <script>hi</script>
                <style>hi</style>
            </div>
            ",
            &[
                (0, vec![("tag", "div")]),
                (0, vec![("tag", "script")]),
                (0, vec![("tag", "script")]),
                (0, vec![("tag", "style")]),
                (0, vec![("tag", "style")]),
                (0, vec![("tag", "div")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_different_tokens_with_the_same_string_value() {
    allocations::record(|| {
        // In Rust, there are two '<' tokens: one for the binary operator,
        // and one with higher precedence for generics.
        let language = get_language("rust");
        let query = Query::new(
            language,
            r#"
                "<" @less
                ">" @greater
                "#,
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "const A: B<C> = d < e || f > g;",
            &[
                (0, vec![("less", "<")]),
                (1, vec![("greater", ">")]),
                (0, vec![("less", "<")]),
                (1, vec![("greater", ">")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_with_too_many_permutations_to_track() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
            (array (identifier) @pre (identifier) @post)
        ",
        )
        .unwrap();

        let mut source = "hello, ".repeat(50);
        source.insert(0, '[');
        source.push_str("];");

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), to_callback(&source));

        // For this pathological query, some match permutations will be dropped.
        // Just check that a subset of the results are returned, and crash or
        // leak occurs.
        assert_eq!(
            collect_matches(matches, &query, source.as_str())[0],
            (0, vec![("pre", "hello"), ("post", "hello")]),
        );
    });
}

#[test]
fn test_query_matches_with_anonymous_tokens() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            ";" @punctuation
            "&&" @operator
            "#,
        )
        .unwrap();

        assert_query_matches(
            language,
            &query,
            "foo(a && b);",
            &[
                (1, vec![("operator", "&&")]),
                (0, vec![("punctuation", ";")]),
            ],
        );
    });
}

#[test]
fn test_query_matches_within_byte_range() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(language, "(identifier) @element").unwrap();

        let source = "[a, b, c, d, e, f, g]";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();

        let mut cursor = QueryCursor::new();
        let matches =
            cursor
                .set_byte_range(5, 15)
                .matches(&query, tree.root_node(), to_callback(source));

        assert_eq!(
            collect_matches(matches, &query, source),
            &[
                (0, vec![("element", "c")]),
                (0, vec![("element", "d")]),
                (0, vec![("element", "e")]),
            ]
        );
    });
}

#[test]
fn test_query_matches_different_queries_same_cursor() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query1 = Query::new(
            language,
            "
            (array (identifier) @id1)
        ",
        )
        .unwrap();
        let query2 = Query::new(
            language,
            "
            (array (identifier) @id1)
            (pair (identifier) @id2)
        ",
        )
        .unwrap();
        let query3 = Query::new(
            language,
            "
            (array (identifier) @id1)
            (pair (identifier) @id2)
            (parenthesized_expression (identifier) @id3)
        ",
        )
        .unwrap();

        let source = "[a, {b: b}, (c)];";

        let mut parser = Parser::new();
        let mut cursor = QueryCursor::new();

        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();

        let matches = cursor.matches(&query1, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query1, source),
            &[(0, vec![("id1", "a")]),]
        );

        let matches = cursor.matches(&query3, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query3, source),
            &[
                (0, vec![("id1", "a")]),
                (1, vec![("id2", "b")]),
                (2, vec![("id3", "c")]),
            ]
        );

        let matches = cursor.matches(&query2, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query2, source),
            &[(0, vec![("id1", "a")]), (1, vec![("id2", "b")]),]
        );
    });
}

#[test]
fn test_query_matches_with_multiple_captures_on_a_node() {
    allocations::record(|| {
        let language = get_language("javascript");
        let mut query = Query::new(
            language,
            "(function_declaration
                (identifier) @name1 @name2 @name3
                (statement_block) @body1 @body2)",
        )
        .unwrap();

        let source = "function foo() { return 1; }";
        let mut parser = Parser::new();
        let mut cursor = QueryCursor::new();

        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();

        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query, source),
            &[(
                0,
                vec![
                    ("name1", "foo"),
                    ("name2", "foo"),
                    ("name3", "foo"),
                    ("body1", "{ return 1; }"),
                    ("body2", "{ return 1; }"),
                ]
            ),]
        );

        // disabling captures still works when there are multiple captures on a
        // single node.
        query.disable_capture("name2");
        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query, source),
            &[(
                0,
                vec![
                    ("name1", "foo"),
                    ("name3", "foo"),
                    ("body1", "{ return 1; }"),
                    ("body2", "{ return 1; }"),
                ]
            ),]
        );
    });
}

#[test]
fn test_query_captures_basic() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (pair
              key: * @method.def
              (function
                name: (identifier) @method.alias))

            (variable_declarator
              name: * @function.def
              value: (function
                name: (identifier) @function.alias))

            ":" @delimiter
            "=" @operator
            "#,
        )
        .unwrap();

        let source = "
          a({
            bc: function de() {
              const fg = function hi() {}
            },
            jk: function lm() {
              const no = function pq() {}
            },
          });
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));

        assert_eq!(
            collect_matches(matches, &query, source),
            &[
                (2, vec![("delimiter", ":")]),
                (0, vec![("method.def", "bc"), ("method.alias", "de")]),
                (3, vec![("operator", "=")]),
                (1, vec![("function.def", "fg"), ("function.alias", "hi")]),
                (2, vec![("delimiter", ":")]),
                (0, vec![("method.def", "jk"), ("method.alias", "lm")]),
                (3, vec![("operator", "=")]),
                (1, vec![("function.def", "no"), ("function.alias", "pq")]),
            ],
        );

        let captures = cursor.captures(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_captures(captures, &query, source),
            &[
                ("method.def", "bc"),
                ("delimiter", ":"),
                ("method.alias", "de"),
                ("function.def", "fg"),
                ("operator", "="),
                ("function.alias", "hi"),
                ("method.def", "jk"),
                ("delimiter", ":"),
                ("method.alias", "lm"),
                ("function.def", "no"),
                ("operator", "="),
                ("function.alias", "pq"),
            ]
        );
    });
}

#[test]
fn test_query_captures_with_text_conditions() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            ((identifier) @constant
             (match? @constant "^[A-Z]{2,}$"))

             ((identifier) @constructor
              (match? @constructor "^[A-Z]"))

            ((identifier) @function.builtin
             (eq? @function.builtin "require"))

             (identifier) @variable
            "#,
        )
        .unwrap();

        let source = "
          const ab = require('./ab');
          new Cd(EF);
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let captures = cursor.captures(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_captures(captures, &query, source),
            &[
                ("variable", "ab"),
                ("function.builtin", "require"),
                ("variable", "require"),
                ("constructor", "Cd"),
                ("variable", "Cd"),
                ("constant", "EF"),
                ("constructor", "EF"),
                ("variable", "EF"),
            ],
        );
    });
}

#[test]
fn test_query_captures_with_predicates() {
    allocations::record(|| {
        let language = get_language("javascript");

        let query = Query::new(
            language,
            r#"
            ((call_expression (identifier) @foo)
             (set! name something)
             (set! cool)
             (something! @foo omg))

            ((property_identifier) @bar
             (is? cool)
             (is-not? name something))"#,
        )
        .unwrap();

        assert_eq!(
            query.property_settings(0),
            &[
                QueryProperty::new("name", Some("something"), None),
                QueryProperty::new("cool", None, None),
            ]
        );
        assert_eq!(
            query.general_predicates(0),
            &[QueryPredicate {
                operator: "something!".to_string().into_boxed_str(),
                args: vec![
                    QueryPredicateArg::Capture(0),
                    QueryPredicateArg::String("omg".to_string().into_boxed_str()),
                ],
            },]
        );
        assert_eq!(query.property_settings(1), &[]);
        assert_eq!(query.property_predicates(0), &[]);
        assert_eq!(
            query.property_predicates(1),
            &[
                (QueryProperty::new("cool", None, None), true),
                (QueryProperty::new("name", Some("something"), None), false),
            ]
        );
    });
}

#[test]
fn test_query_captures_with_quoted_predicate_args() {
    allocations::record(|| {
        let language = get_language("javascript");

        // Double-quoted strings can contain:
        // * special escape sequences like \n and \r
        // * escaped double quotes with \*
        // * literal backslashes with \\
        let query = Query::new(
            language,
            r#"
            ((call_expression (identifier) @foo)
             (set! one "\"something\ngreat\""))

            ((identifier)
             (set! two "\\s(\r?\n)*$"))

            ((function_declaration)
             (set! three "\"something\ngreat\""))
            "#,
        )
        .unwrap();

        assert_eq!(
            query.property_settings(0),
            &[QueryProperty::new(
                "one",
                Some("\"something\ngreat\""),
                None
            )]
        );
        assert_eq!(
            query.property_settings(1),
            &[QueryProperty::new("two", Some("\\s(\r?\n)*$"), None)]
        );
        assert_eq!(
            query.property_settings(2),
            &[QueryProperty::new(
                "three",
                Some("\"something\ngreat\""),
                None
            )]
        );
    });
}

#[test]
fn test_query_captures_with_duplicates() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (variable_declarator
                name: (identifier) @function
                value: (function))

            (identifier) @variable
            "#,
        )
        .unwrap();

        let source = "
          var x = function() {};
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let captures = cursor.captures(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_captures(captures, &query, source),
            &[("function", "x"), ("variable", "x"),],
        );
    });
}

#[test]
fn test_query_captures_with_many_nested_results_without_fields() {
    allocations::record(|| {
        let language = get_language("javascript");

        // Search for key-value pairs whose values are anonymous functions.
        let query = Query::new(
            language,
            r#"
            (pair
              key: * @method-def
              (arrow_function))

            ":" @colon
            "," @comma
            "#,
        )
        .unwrap();

        // The `pair` node for key `y` does not match any pattern, but inside of
        // its value, it contains many other `pair` nodes that do match the pattern.
        // The match for the *outer* pair should be terminated *before* descending into
        // the object value, so that we can avoid needing to buffer all of the inner
        // matches.
        let method_count = 50;
        let mut source = "x = { y: {\n".to_owned();
        for i in 0..method_count {
            writeln!(&mut source, "    method{}: $ => null,", i).unwrap();
        }
        source.push_str("}};\n");

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let captures = cursor.captures(&query, tree.root_node(), to_callback(&source));
        let captures = collect_captures(captures, &query, &source);

        assert_eq!(
            &captures[0..13],
            &[
                ("colon", ":"),
                ("method-def", "method0"),
                ("colon", ":"),
                ("comma", ","),
                ("method-def", "method1"),
                ("colon", ":"),
                ("comma", ","),
                ("method-def", "method2"),
                ("colon", ":"),
                ("comma", ","),
                ("method-def", "method3"),
                ("colon", ":"),
                ("comma", ","),
            ]
        );

        // Ensure that we don't drop matches because of needing to buffer too many.
        assert_eq!(captures.len(), 1 + 3 * method_count);
    });
}

#[test]
fn test_query_captures_with_many_nested_results_with_fields() {
    allocations::record(|| {
        let language = get_language("javascript");

        // Search expressions like `a ? a.b : null`
        let query = Query::new(
            language,
            r#"
            ((ternary_expression
                condition: (identifier) @left
                consequence: (member_expression
                    object: (identifier) @right)
                alternative: (null))
             (eq? @left @right))
            "#,
        )
        .unwrap();

        // The outer expression does not match the pattern, but the consequence of the ternary
        // is an object that *does* contain many occurences of the pattern.
        let count = 50;
        let mut source = "a ? {".to_owned();
        for i in 0..count {
            writeln!(&mut source, "  x: y{} ? y{}.z : null,", i, i).unwrap();
        }
        source.push_str("} : null;\n");

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let captures = cursor.captures(&query, tree.root_node(), to_callback(&source));
        let captures = collect_captures(captures, &query, &source);

        assert_eq!(
            &captures[0..20],
            &[
                ("left", "y0"),
                ("right", "y0"),
                ("left", "y1"),
                ("right", "y1"),
                ("left", "y2"),
                ("right", "y2"),
                ("left", "y3"),
                ("right", "y3"),
                ("left", "y4"),
                ("right", "y4"),
                ("left", "y5"),
                ("right", "y5"),
                ("left", "y6"),
                ("right", "y6"),
                ("left", "y7"),
                ("right", "y7"),
                ("left", "y8"),
                ("right", "y8"),
                ("left", "y9"),
                ("right", "y9"),
            ]
        );

        // Ensure that we don't drop matches because of needing to buffer too many.
        assert_eq!(captures.len(), 2 * count);
    });
}

#[test]
fn test_query_captures_with_too_many_nested_results() {
    allocations::record(|| {
        let language = get_language("javascript");

        // Search for method calls in general, and also method calls with a template string
        // in place of an argument list (aka "tagged template strings") in particular.
        //
        // This second pattern, which looks for the tagged template strings, is expensive to
        // use with the `captures()` method, because:
        // 1. When calling `captures`, all of the captures must be returned in order of their
        //    appearance.
        // 2. This pattern captures the root `call_expression`.
        // 3. This pattern's result also depends on the final child (the template string).
        // 4. In between the `call_expression` and the possible `template_string`, there can
        //    be an arbitrarily deep subtree.
        //
        // This means that, if any patterns match *after* the initial `call_expression` is
        // captured, but before the final `template_string` is found, those matches must
        // be buffered, in order to prevent captures from being returned out-of-order.
        let query = Query::new(
            language,
            r#"
            ;; easy 👇
            (call_expression
              function: (member_expression
                property: (property_identifier) @method-name))

            ;; hard 👇
            (call_expression
              function: (member_expression
                property: (property_identifier) @template-tag)
              arguments: (template_string)) @template-call
            "#,
        )
        .unwrap();

        // There are a *lot* of matches in between the beginning of the outer `call_expression`
        // (the call to `a(...).f`), which starts at the beginning of the file, and the final
        // template string, which occurs at the end of the file. The query algorithm imposes a
        // limit on the total number of matches which can be buffered at a time. But we don't
        // want to neglect the inner matches just because of the expensive outer match, so we
        // abandon the outer match (which would have captured `f` as a `template-tag`).
        let source = "
        a(b => {
            b.c0().d0 `😄`;
            b.c1().d1 `😄`;
            b.c2().d2 `😄`;
            b.c3().d3 `😄`;
            b.c4().d4 `😄`;
            b.c5().d5 `😄`;
            b.c6().d6 `😄`;
            b.c7().d7 `😄`;
            b.c8().d8 `😄`;
            b.c9().d9 `😄`;
        }).e().f ``;
        "
        .trim();

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let captures = cursor.captures(&query, tree.root_node(), to_callback(&source));
        let captures = collect_captures(captures, &query, &source);

        assert_eq!(
            &captures[0..4],
            &[
                ("template-call", "b.c0().d0 `😄`"),
                ("method-name", "c0"),
                ("method-name", "d0"),
                ("template-tag", "d0"),
            ]
        );
        assert_eq!(
            &captures[36..40],
            &[
                ("template-call", "b.c9().d9 `😄`"),
                ("method-name", "c9"),
                ("method-name", "d9"),
                ("template-tag", "d9"),
            ]
        );
        assert_eq!(
            &captures[40..],
            &[("method-name", "e"), ("method-name", "f"),]
        );
    });
}

#[test]
fn test_query_captures_ordered_by_both_start_and_end_positions() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (call_expression) @call
            (member_expression) @member
            (identifier) @variable
            "#,
        )
        .unwrap();

        let source = "
          a.b(c.d().e).f;
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let captures = cursor.captures(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_captures(captures, &query, source),
            &[
                ("member", "a.b(c.d().e).f"),
                ("call", "a.b(c.d().e)"),
                ("member", "a.b"),
                ("variable", "a"),
                ("member", "c.d().e"),
                ("call", "c.d()"),
                ("member", "c.d"),
                ("variable", "c"),
            ],
        );
    });
}

#[test]
fn test_query_captures_with_matches_removed() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (binary_expression
                left: (identifier) @left
                operator: * @op
                right: (identifier) @right)
            "#,
        )
        .unwrap();

        let source = "
          a === b && c > d && e < f;
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();

        let mut captured_strings = Vec::new();
        for (m, i) in cursor.captures(&query, tree.root_node(), to_callback(source)) {
            let capture = m.captures[i];
            let text = capture.node.utf8_text(source.as_bytes()).unwrap();
            if text == "a" {
                m.remove();
                continue;
            }
            captured_strings.push(text);
        }

        assert_eq!(captured_strings, &["c", ">", "d", "e", "<", "f",]);
    });
}

#[test]
fn test_query_captures_and_matches_iterators_are_fused() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (comment) @comment
            "#,
        )
        .unwrap();

        let source = "
          // one
          // two
          // three
          /* unfinished
        ";

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), to_callback(source));

        assert_eq!(captures.next().unwrap().0.captures[0].index, 0);
        assert_eq!(captures.next().unwrap().0.captures[0].index, 0);
        assert_eq!(captures.next().unwrap().0.captures[0].index, 0);
        assert!(captures.next().is_none());
        assert!(captures.next().is_none());
        assert!(captures.next().is_none());
        drop(captures);

        let mut matches = cursor.matches(&query, tree.root_node(), to_callback(source));
        assert_eq!(matches.next().unwrap().captures[0].index, 0);
        assert_eq!(matches.next().unwrap().captures[0].index, 0);
        assert_eq!(matches.next().unwrap().captures[0].index, 0);
        assert!(matches.next().is_none());
        assert!(matches.next().is_none());
        assert!(matches.next().is_none());
    });
}

#[test]
fn test_query_start_byte_for_pattern() {
    let language = get_language("javascript");

    let patterns_1 = r#"
        "+" @operator
        "-" @operator
        "*" @operator
        "=" @operator
        "=>" @operator
    "#
    .trim_start();

    let patterns_2 = "
        (identifier) @a
        (string) @b
    "
    .trim_start();

    let patterns_3 = "
        ((identifier) @b (match? @b i))
        (function_declaration name: (identifier) @c)
        (method_definition name: (identifier) @d)
    "
    .trim_start();

    let mut source = String::new();
    source += patterns_1;
    source += patterns_2;
    source += patterns_3;

    let query = Query::new(language, &source).unwrap();

    assert_eq!(query.start_byte_for_pattern(0), 0);
    assert_eq!(query.start_byte_for_pattern(5), patterns_1.len());
    assert_eq!(
        query.start_byte_for_pattern(7),
        patterns_1.len() + patterns_2.len()
    );
}

#[test]
fn test_query_capture_names() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            r#"
            (if_statement
              condition: (binary_expression
                left: * @left-operand
                operator: "||"
                right: * @right-operand)
              consequence: (statement_block) @body)

            (while_statement
              condition:* @loop-condition)
            "#,
        )
        .unwrap();

        assert_eq!(
            query.capture_names(),
            &[
                "left-operand".to_string(),
                "right-operand".to_string(),
                "body".to_string(),
                "loop-condition".to_string(),
            ]
        );
    });
}

#[test]
fn test_query_with_no_patterns() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(language, "").unwrap();
        assert!(query.capture_names().is_empty());
        assert_eq!(query.pattern_count(), 0);
    });
}

#[test]
fn test_query_comments() {
    allocations::record(|| {
        let language = get_language("javascript");
        let query = Query::new(
            language,
            "
                ; this is my first comment
                ; i have two comments here
                (function_declaration
                    ; there is also a comment here
                    ; and here
                    name: (identifier) @fn-name)",
        )
        .unwrap();

        let source = "function one() { }";
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query, source),
            &[(0, vec![("fn-name", "one")]),],
        );
    });
}

#[test]
fn test_query_disable_pattern() {
    allocations::record(|| {
        let language = get_language("javascript");
        let mut query = Query::new(
            language,
            "
                (function_declaration
                    name: (identifier) @name)
                (function_declaration
                    body: (statement_block) @body)
                (class_declaration
                    name: (identifier) @name)
                (class_declaration
                    body: (class_body) @body)
            ",
        )
        .unwrap();

        // disable the patterns that match names
        query.disable_pattern(0);
        query.disable_pattern(2);

        let source = "class A { constructor() {} } function b() { return 1; }";
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), to_callback(source));
        assert_eq!(
            collect_matches(matches, &query, source),
            &[
                (3, vec![("body", "{ constructor() {} }")]),
                (1, vec![("body", "{ return 1; }")]),
            ],
        );
    });
}

fn assert_query_matches(
    language: Language,
    query: &Query,
    source: &str,
    expected: &[(usize, Vec<(&str, &str)>)],
) {
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node(), to_callback(source));
    assert_eq!(collect_matches(matches, &query, source), expected);
}

fn collect_matches<'a>(
    matches: impl Iterator<Item = QueryMatch<'a>>,
    query: &'a Query,
    source: &'a str,
) -> Vec<(usize, Vec<(&'a str, &'a str)>)> {
    matches
        .map(|m| {
            (
                m.pattern_index,
                format_captures(m.captures.iter().cloned(), query, source),
            )
        })
        .collect()
}

fn collect_captures<'a>(
    captures: impl Iterator<Item = (QueryMatch<'a>, usize)>,
    query: &'a Query,
    source: &'a str,
) -> Vec<(&'a str, &'a str)> {
    format_captures(captures.map(|(m, i)| m.captures[i]), query, source)
}

fn format_captures<'a>(
    captures: impl Iterator<Item = QueryCapture<'a>>,
    query: &'a Query,
    source: &'a str,
) -> Vec<(&'a str, &'a str)> {
    captures
        .map(|capture| {
            (
                query.capture_names()[capture.index as usize].as_str(),
                capture.node.utf8_text(source.as_bytes()).unwrap(),
            )
        })
        .collect()
}

fn to_callback<'a>(source: &'a str) -> impl Fn(Node) -> &'a [u8] {
    move |n| &source.as_bytes()[n.byte_range()]
}
