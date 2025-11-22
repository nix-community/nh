use std::fmt::Display;

#[derive(Debug, Clone)]
pub struct Value<'v> {
  pub inner: &'v serde_json::Value,
  get_stack: Vec<String>,
}

#[derive(Debug)]
pub struct Error {
  get_stack: Vec<String>,
}

impl Display for Error {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "Failed to index json value with ")?;

    for (i, elem) in self.get_stack.iter().enumerate() {
      if i != 0 {
        write!(f, ".")?;
      }
      write!(f, "{elem}")?;
    }

    Ok(())
  }
}

impl std::error::Error for Error {}

#[allow(unused)]
impl<'v> Value<'v> {
  #[must_use]
  pub const fn new(value: &'v serde_json::Value) -> Self {
    Self {
      inner:     value,
      get_stack: vec![],
    }
  }

  pub fn get(&self, index: &str) -> Result<Self, Error> {
    let mut get_stack = self.get_stack.clone();
    get_stack.push(index.to_owned());

    match self.inner.get(index) {
      Some(value) => {
        Ok(Self {
          inner: value,
          get_stack,
        })
      },
      None => Err(Error { get_stack }),
    }
  }
}

#[test]
fn test_value() {
  let input = serde_json::json!({
      "foo": {
          "bar": "baz",
          "some": {
              "other": "value"
          }
      }
  });

  let i = Value::new(&input);

  assert!(i.get("foo").is_ok());
  assert!(i.get("foo_bad").is_err());
  assert!(i.get("foo").expect("foo should exist").get("bar").is_ok());
  assert!(
    i.get("foo")
      .expect("foo should exist")
      .get("some")
      .expect("some should exist")
      .get("other_bad")
      .is_err()
  );
  assert!(
    i.get("foo")
      .expect("foo should exist")
      .get("some")
      .expect("some should exist")
      .get("other")
      .is_ok()
  );
}
