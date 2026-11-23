//! Name newtypes: a string that means one kind of thing (a ship's name, a group, an owner) cannot be passed for another.
//! Serde is transparent, so JSON and saves read and write the plain string as before.

macro_rules! name_newtype {
    ($(#[$m:meta])* $t:ident) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $t(String);
        impl $t {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl From<&str> for $t {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
        impl From<String> for $t {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl std::borrow::Borrow<str> for $t {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
        impl std::ops::Deref for $t {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
        impl PartialEq<str> for $t {
            fn eq(&self, o: &str) -> bool {
                self.0 == o
            }
        }
        impl PartialEq<&str> for $t {
            fn eq(&self, o: &&str) -> bool {
                self.0 == *o
            }
        }
    };
}
pub(crate) use name_newtype;

name_newtype!(
    /// The rules' player name of a ship's owner, and the key of its fleet (`MissionShip::owner`, `Fleets`). Changes with
    /// 'Set Group/Unit Owner'.
    Owner
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_reads_and_writes_the_plain_json_string() {
        let o: Owner = serde_json::from_str("\"Player\"").unwrap();
        assert_eq!(o, "Player");
        assert_eq!(serde_json::to_string(&o).unwrap(), "\"Player\"");
        let m: std::collections::BTreeMap<Owner, u32> = serde_json::from_str(r#"{"Pirates":2,"Player":1}"#).unwrap();
        assert_eq!(m.get("Player"), Some(&1));
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"Pirates":2,"Player":1}"#);
    }
}
