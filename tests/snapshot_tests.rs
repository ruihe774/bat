mod tester;

macro_rules! snapshot_tests {
    ($($test_name: ident: $style: expr,)*) => {
        $(
            #[test]
            fn $test_name() {
                let bat_tester = tester::BatTester::default();
                bat_tester.test_snapshot(stringify!($test_name), $style);
            }
        )*
    };
}

snapshot_tests! {
    changes:                     "changes",
    grid:                        "grid",
    header:                      "header",
    numbers:                     "numbers",
    rule:                        "rule",
    grid_header:                 "grid,header",
    grid_numbers:                "grid,numbers",
    grid_rule:                   "grid,rule",
    header_numbers:              "header,numbers",
    header_rule:                 "header,rule",
    changes_header_numbers:      "changes,header,numbers",
    changes_header_rule:         "changes,header,rule",
    grid_header_numbers:         "grid,header,numbers",
    grid_header_rule:            "grid,header,rule",
    header_numbers_rule:         "header,numbers,rule",
    plain:                       "plain",
}
