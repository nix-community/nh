use crate::installable::{Installable, join_attribute, parse_attribute};
use crate::testing::harness::*;
use crate::testing::prop::*;
use crate::init_prop_test;
use proptest::prelude::*;
use std::path::PathBuf;

/// Property tests for Installable conversions
#[cfg(test)]
mod prop_tests {
    use super::*;

    /// Test that Installable::Flake properly converts to arguments
    #[test]
    fn prop_installable_flake_to_args() {
        // Initialize property test environment with default configuration
        init_prop_test!();

        proptest!(|(
            reference in flake_reference_strategy(),
            attribute in prop::collection::vec("[a-zA-Z0-9_]{1,10}", 0..5)
        )| {
            // Create a flake installable
            let flake = Installable::Flake {
                reference: reference.clone(),
                attribute: attribute.iter().map(String::from).collect(),
            };
            
            // Convert to args
            let args = flake.to_args();
            
            // Ensure we got exactly one argument
            prop_assert_eq!(args.len(), 1);
            
            // If attribute path is empty, the result should just be the reference
            if attribute.is_empty() {
                prop_assert_eq!(&args[0], &format!("{}#", reference));
            } else {
                // Split the result back to verify
                let parts: Vec<&str> = args[0].splitn(2, '#').collect();
                prop_assert_eq!(parts.len(), 2);
                prop_assert_eq!(parts[0], &reference);
                
                // Re-parse the attribute path to verify it matches
                let joined_attr = join_attribute(attribute.iter().map(String::from));
                prop_assert_eq!(parts[1], &joined_attr);
            }
        });
    }

    /// Test that Installable::File properly converts to arguments
    #[test]
    fn prop_installable_file_to_args() {
        // Initialize property test environment
        init_prop_test!();

        proptest!(|(
            // Generate valid file paths
            path in file_path_strategy(),
            // Generate attribute paths with possible dots
            attribute in prop::collection::vec(
                prop::string::string_regex("[a-zA-Z0-9_\\.]{1,10}").unwrap(), 
                0..5
            )
        )| {
            // Create a file installable
            let file = Installable::File {
                path: path.clone(),
                attribute: attribute.clone(),
            };
            
            // Convert to args
            let args = file.to_args();
            
            // Check result structure
            prop_assert!(args.len() >= 2, "File installable should have at least --file and path arguments");
            prop_assert_eq!(&args[0], "--file");
            prop_assert_eq!(&args[1], &path.to_string_lossy().to_string());
            
            // Check attribute conversion
            if !attribute.is_empty() {
                prop_assert_eq!(args.len(), 3);
                prop_assert_eq!(&args[2], &join_attribute(&attribute));
            }
        });
    }

    /// Property test for Installable::Expression conversion to args
    #[test]
    fn prop_installable_expression_to_args() {
        init_prop_test!();

        proptest!(|(
            // Generate expression strings
            expr in "[a-zA-Z0-9_\\-\\.\\{\\}\\$]{1,50}",
            // Generate attribute paths
            attribute in prop::collection::vec("[a-zA-Z0-9_]{1,10}", 0..5)
        )| {
            // Create an expression installable
            let expression = Installable::Expression {
                expression: expr.clone(),
                attribute: attribute.iter().map(String::from).collect(),
            };
            
            // Convert to args
            let args = expression.to_args();
            
            // Check result structure
            prop_assert!(args.len() >= 2, "Expression installable should have at least --expr and the expression arguments");
            prop_assert_eq!(&args[0], "--expr");
            prop_assert_eq!(&args[1], &expr);
            
            // Check attribute conversion
            if !attribute.is_empty() {
                prop_assert_eq!(args.len(), 3);
                prop_assert_eq!(&args[2], &join_attribute(attribute.iter().map(String::from)));
            }
        });
    }

    /// Property test for Installable::Store conversion to args
    #[test]
    fn prop_installable_store_to_args() {
        init_prop_test!();

        proptest!(|(
            // Generate store paths
            store_path in "/nix/store/[a-z0-9]{32}-[a-zA-Z0-9_\\-\\.]{1,50}"
        )| {
            // Create a store installable
            let store = Installable::Store {
                path: PathBuf::from(&store_path),
            };
            
            // Convert to args
            let args = store.to_args();
            
            // Check result
            prop_assert_eq!(args.len(), 1, "Store installable should have exactly one argument");
            prop_assert_eq!(&args[0], &store_path);
        });
    }

    /// Property test: join_attribute followed by parse_attribute should maintain original values
    #[test]
    fn prop_join_parse_attribute_roundtrip() {
        init_prop_test!();

        proptest!(|(
            // Generate simple attribute strings
            attributes in prop::collection::vec("[a-zA-Z0-9_]{1,10}", 1..5)
        )| {
            // Convert strings to String for the test
            let attribute_strings: Vec<String> = attributes.iter().map(String::from).collect();
            
            // Join them together
            let joined = join_attribute(&attribute_strings);
            
            // Parse them back
            let parsed = parse_attribute(joined);
            
            // Should get the same values back
            prop_assert_eq!(attribute_strings.len(), parsed.len());
            for (orig, parsed) in attribute_strings.iter().zip(parsed.iter()) {
                prop_assert_eq!(orig, parsed);
            }
        });
    }

    /// Property test: join_attribute should properly handle attributes with dots
    #[test]
    fn prop_join_attribute_with_dots() {
        init_prop_test!();

        proptest!(|(
            // Generate an attribute path with some components containing dots
            components in prop::collection::vec(prop::string::string_regex("[a-zA-Z0-9_]{1,10}").unwrap(), 1..3),
            dotted_component in prop::string::string_regex("[a-zA-Z0-9_]+\\.[a-zA-Z0-9_]+").unwrap()
        )| {
            // Create test data with a mix of regular components and one with dots
            let mut attribute_strings: Vec<String> = components.iter().map(String::from).collect();
            attribute_strings.push(dotted_component.clone());
            
            // Join them together
            let joined = join_attribute(&attribute_strings);
            
            // The component with dots should be quoted
            prop_assert!(joined.contains(&format!("\"{}\"", dotted_component)));
            
            // Parse them back
            let parsed = parse_attribute(joined);
            
            // Should get the same values back
            prop_assert_eq!(attribute_strings.len(), parsed.len());
            for (orig, parsed) in attribute_strings.iter().zip(parsed.iter()) {
                prop_assert_eq!(orig, parsed);
            }
        });
    }
}