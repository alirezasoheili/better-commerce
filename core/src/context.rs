#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Principal {
    Anonymous,
    System { actor: String },
    Authenticated { subject: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestContext {
    principal: Principal,
}

impl RequestContext {
    pub fn with_principal(principal: Principal) -> Self {
        Self { principal }
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::with_principal(Principal::Anonymous)
    }
}

#[cfg(test)]
mod tests {
    use super::{Principal, RequestContext};

    #[test]
    fn new_request_context_is_anonymous() {
        assert_eq!(RequestContext::default().principal(), &Principal::Anonymous);
    }

    #[test]
    fn request_context_can_carry_an_explicit_principal() {
        let context = RequestContext::with_principal(Principal::Authenticated {
            subject: "operator-123".to_owned(),
        });

        assert_eq!(
            context.principal(),
            &Principal::Authenticated {
                subject: "operator-123".to_owned()
            }
        );
    }
}
