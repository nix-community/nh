use proptest::prelude::*;

use crate::{
    init_prop_test,
    init_test,
    installable::parse_attribute,
    testing::harness::*,
};

/// Unit tests for the Installable module
#[cfg(test)]
mod tests {
    use super::*;

    /// Test that parse_attribute correctly handles various inputs
    #[test]
    fn test_parse_attribute_examples() {
        // Initialize test environment
        init_test!();

        // Example test cases
        assert_eq!(parse_attribute("").len(), 0);
        assert_eq!(parse_attribute("foo"), vec!["foo".to_string()]);
        assert_eq!(parse_attribute("foo.bar"), vec![
            "foo".to_string(),
            "bar".to_string()
        ]);
        assert_eq!(parse_attribute(r#"foo."bar.baz""#), vec![
            "foo".to_string(),
            "bar.baz".to_string()
        ]);
        assert_eq!(parse_attribute(r#"nixosConfigurations.myhost"#), vec![
            "nixosConfigurations".to_string(),
            "myhost".to_string()
        ]);
    }

    /// Test environment variable handling with EnvContext
    #[test]
    fn test_env_var_handling() {
        init_test!();

        let mut ctx = EnvContext::new();

        // Set test environment variables
        ctx.set_var("NH_TEST_FLAKE", "test-flake-value")
            .set_var("NH_TEST_ATTR", "test.attr.value");

        // Run test with environment variables set
        ctx.run(|| {
            assert_eq!(std::env::var("NH_TEST_FLAKE").unwrap(), "test-flake-value");
            assert_eq!(std::env::var("NH_TEST_ATTR").unwrap(), "test.attr.value");

            // Test parse_attribute with the environment variable
            let attr = parse_attribute(&std::env::var("NH_TEST_ATTR").unwrap());
            assert_eq!(attr, vec![
                "test".to_string(),
                "attr".to_string(),
                "value".to_string()
            ]);
        });

        // Environment should be restored after test
        assert!(std::env::var("NH_TEST_FLAKE").is_err());
        assert!(std::env::var("NH_TEST_ATTR").is_err());
    }

    /// Property tests for parse_attribute function
    #[test]
    fn prop_parse_attribute_properties() {
        // Initialize proptest environment with default configuration
        init_prop_test!();

        // Property 1: Empty string should result in empty vec
        proptest!(|(empty_string in "")| {
            let result = parse_attribute(&empty_string);
            prop_assert!(result.is_empty());
        });

        // Property 2: A single identifier without dots should be returned as single
        // element vec
        proptest!(|(single_ident in "[a-zA-Z_][a-zA-Z0-9_]*")| {
            let result = parse_attribute(&single_ident);
            prop_assert_eq!(result.len(), 1);
            prop_assert_eq!(&result[0], &single_ident);
        });

        // Property 3: Simple dot notation should split into components
        proptest!(|(
            ident1 in "[a-zA-Z_][a-zA-Z0-9_]*",
            ident2 in "[a-zA-Z_][a-zA-Z0-9_]*"
        )| {
            let input = format!("{}.{}", ident1, ident2);
            let result = parse_attribute(&input);

            prop_assert_eq!(result.len(), 2);
            prop_assert_eq!(&result[0], &ident1);
            prop_assert_eq!(&result[1], &ident2);
        });

        // Property 4: Quoted strings in dot notation should preserve dots within quotes
        proptest!(|(
            ident1 in "[a-zA-Z_][a-zA-Z0-9_]*",
            inner_text in "[a-zA-Z0-9_\\.]{1,20}"
        )| {
            let input = format!("{}.\"{}\""   , ident1, inner_text);
            let result = parse_attribute(&input);

            prop_assert_eq!(result.len(), 2);
            prop_assert_eq!(&result[0], &ident1);
            prop_assert_eq!(&result[1], &inner_text);
        });

        // Property 5: Multiple dots should result in multiple elements
        proptest!(|(
            components in prop::collection::vec("[a-zA-Z_][a-zA-Z0-9_]*", 1..5)
        )| {
            let input = components.join(".");
            let result = parse_attribute(&input);

            prop_assert_eq!(result.len(), components.len());
            for (i, component) in components.iter().enumerate() {
                prop_assert_eq!(&result[i], component);
            }
        });

        // Property 6: Mixed quoted and unquoted segments with whitespace
        proptest!(|(
            ident1 in "[a-zA-Z_][a-zA-Z0-9_]*",
            quoted in "[a-zA-Z0-9_\\.]{1,20}",
            ident3 in "[a-zA-Z_][a-zA-Z0-9_]*"
        )| {
            // Using various whitespace patterns to test robustness
            let input = format!("{}  . \"{}\" .  {}", ident1, quoted, ident3);

            let result = parse_attribute(&input);

            // Debug logging for failures
            if result.len() != 3 {
                tracing::debug!("Parse failure: input='{}', result={:?}", input, result);
            }

            prop_assert_eq!(result.len(), 3,
                "Expected 3 components from '{}', got {}: {:?}",
                input, result.len(), result);

            prop_assert_eq!(&result[0], &ident1);
            prop_assert_eq!(&result[1], &quoted);
            prop_assert_eq!(&result[2], &ident3);
        });

        // Property 7: Handle whitespace at beginning and end
        proptest!(|(
            ident1 in "[a-zA-Z_][a-zA-Z0-9_]*",
            ident2 in "[a-zA-Z_][a-zA-Z0-9_]*"
        )| {
            let input = format!("  {}  .  {}  ", ident1, ident2);
            let result = parse_attribute(&input);

            prop_assert_eq!(result.len(), 2);
            prop_assert_eq!(&result[0], &ident1);
            prop_assert_eq!(&result[1], &ident2);
        });
    }
}
