use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Permission {
    pub action: String,
    pub resource_type: String,
    pub in_states: Vec<String>,
}

impl Permission {
    /// Parse shorthand like "view", "transition:UNDERWRITING", "transition:*"
    pub fn from_shorthand(s: &str) -> Self {
        if let Some((action, rest)) = s.split_once(':') {
            Permission {
                action: action.to_string(),
                resource_type: "*".to_string(),
                in_states: vec![rest.to_string()],
            }
        } else {
            Permission {
                action: s.to_string(),
                resource_type: "*".to_string(),
                in_states: vec!["*".to_string()],
            }
        }
    }

    pub fn matches(&self, action: &str, resource_type: &str, state: &str) -> bool {
        let action_match = self.action == "*" || self.action == action;
        let type_match = self.resource_type == "*" || self.resource_type == resource_type;
        let state_match = self.in_states.is_empty()
            || self.in_states.iter().any(|s| s == "*" || s == state);
        action_match && type_match && state_match
    }
}

#[derive(Debug, Clone)]
pub struct RoleDefinition {
    pub name: String,
    pub permissions: Vec<Permission>,
    /// Which data fields this role can see. Empty or ["*"] = all fields visible.
    pub visible_fields: Vec<String>,
}

pub struct RoleRegistry {
    roles: HashMap<String, RoleDefinition>,
}

impl RoleRegistry {
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
        }
    }

    pub fn register(&mut self, role: RoleDefinition) {
        self.roles.insert(role.name.clone(), role);
    }

    pub fn check_permission(
        &self,
        role: &str,
        action: &str,
        resource_type: &str,
        state: &str,
    ) -> bool {
        self.roles
            .get(role)
            .map(|r| r.permissions.iter().any(|p| p.matches(action, resource_type, state)))
            .unwrap_or(false)
    }

    pub fn get_role(&self, name: &str) -> Option<&RoleDefinition> {
        self.roles.get(name)
    }

    pub fn has_role(&self, name: &str) -> bool {
        self.roles.contains_key(name)
    }

    /// Filter a JSON data object to only include fields visible to this role.
    /// Returns all fields if the role has no field restrictions (empty or ["*"]).
    pub fn filter_visible_fields(&self, role: &str, data: &serde_json::Value) -> serde_json::Value {
        let role_def = match self.roles.get(role) {
            Some(r) => r,
            None => return serde_json::Value::Object(serde_json::Map::new()), // Unknown role sees nothing
        };

        // Empty or ["*"] = all fields visible
        if role_def.visible_fields.is_empty()
            || role_def.visible_fields.iter().any(|f| f == "*")
        {
            return data.clone();
        }

        // Filter to only permitted fields
        match data.as_object() {
            Some(map) => {
                let filtered: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .filter(|(key, _)| role_def.visible_fields.iter().any(|f| f == key.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                serde_json::Value::Object(filtered)
            }
            None => data.clone(),
        }
    }
}

impl Default for RoleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> RoleRegistry {
        let mut reg = RoleRegistry::new();
        reg.register(RoleDefinition {
            name: "officer".into(),
            visible_fields: vec![],
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:UNDERWRITING"),
            ],
        });
        reg.register(RoleDefinition {
            name: "manager".into(),
            visible_fields: vec![],
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:*"),
            ],
        });
        reg
    }

    #[test]
    fn test_officer_can_view() {
        let reg = make_registry();
        assert!(reg.check_permission("officer", "view", "loan", "APPLIED"));
    }

    #[test]
    fn test_officer_can_transition_to_underwriting() {
        let reg = make_registry();
        assert!(reg.check_permission("officer", "transition", "loan", "UNDERWRITING"));
    }

    #[test]
    fn test_officer_cannot_transition_to_approved() {
        let reg = make_registry();
        assert!(!reg.check_permission("officer", "transition", "loan", "APPROVED"));
    }

    #[test]
    fn test_manager_can_transition_anywhere() {
        let reg = make_registry();
        assert!(reg.check_permission("manager", "transition", "loan", "APPROVED"));
        assert!(reg.check_permission("manager", "transition", "loan", "DISBURSED"));
    }

    #[test]
    fn test_unknown_role() {
        let reg = make_registry();
        assert!(!reg.check_permission("nobody", "view", "loan", "APPLIED"));
    }

    #[test]
    fn test_permission_shorthand_simple() {
        let p = Permission::from_shorthand("view");
        assert_eq!(p.action, "view");
        assert_eq!(p.in_states, vec!["*"]);
    }

    #[test]
    fn test_permission_shorthand_with_state() {
        let p = Permission::from_shorthand("transition:UNDERWRITING");
        assert_eq!(p.action, "transition");
        assert_eq!(p.in_states, vec!["UNDERWRITING"]);
    }

    #[test]
    fn test_visible_fields_all() {
        let mut reg = RoleRegistry::new();
        reg.register(RoleDefinition {
            name: "admin".into(),
            visible_fields: vec![], // Empty = all fields
            permissions: vec![],
        });

        let data = serde_json::json!({"amount": 100, "pan": "ABCDE1234F", "name": "Acme"});
        let filtered = reg.filter_visible_fields("admin", &data);
        assert_eq!(filtered, data); // All fields visible
    }

    #[test]
    fn test_visible_fields_restricted() {
        let mut reg = RoleRegistry::new();
        reg.register(RoleDefinition {
            name: "viewer".into(),
            visible_fields: vec!["amount".into(), "name".into()],
            permissions: vec![],
        });

        let data = serde_json::json!({"amount": 100, "pan": "ABCDE1234F", "name": "Acme"});
        let filtered = reg.filter_visible_fields("viewer", &data);

        assert_eq!(filtered.get("amount").unwrap(), &serde_json::json!(100));
        assert_eq!(filtered.get("name").unwrap(), &serde_json::json!("Acme"));
        assert!(filtered.get("pan").is_none()); // PII hidden
    }

    #[test]
    fn test_visible_fields_wildcard() {
        let mut reg = RoleRegistry::new();
        reg.register(RoleDefinition {
            name: "super".into(),
            visible_fields: vec!["*".into()],
            permissions: vec![],
        });

        let data = serde_json::json!({"amount": 100, "pan": "ABCDE1234F"});
        let filtered = reg.filter_visible_fields("super", &data);
        assert_eq!(filtered, data);
    }

    #[test]
    fn test_unknown_role_sees_nothing() {
        let reg = RoleRegistry::new();
        let data = serde_json::json!({"secret": "value"});
        let filtered = reg.filter_visible_fields("nonexistent", &data);
        assert_eq!(filtered, serde_json::json!({}));
    }
}
