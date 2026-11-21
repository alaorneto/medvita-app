//! The sign-in state machine of the shell, without GPUI (unit-tested).
//!
//! `Booting → NeedsServer | SignedOut | LoadingProfile`; `SignedOut → SigningIn → LoadingProfile`;
//! `LoadingProfile → SelectingOrganization | Ready | Unavailable`; any signed-in phase goes back to
//! `SignedOut` when the session expires or the person signs out.

use medvita_api::Organization;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    SessionExpired,
    NoActiveOrganization,
    /// Already translated message (sign-in failure, server error…).
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Booting,
    NeedsServer,
    SignedOut,
    SigningIn,
    LoadingProfile,
    /// The profile could not be loaded for a reason other than the session (network, maintenance).
    Unavailable {
        message: String,
    },
    SelectingOrganization,
    Ready {
        organization: Organization,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    ServerMissing,
    Connecting,
    NoSession,
    SessionFound,
    SignInStarted,
    SignInCancelled,
    SignInFailed(String),
    SignedIn,
    ProfileLoaded {
        organizations: Vec<Organization>,
        last: Option<Uuid>,
    },
    ProfileFailed(String),
    OrganizationChosen(Uuid),
    ChangeOrganization,
    SessionExpired,
    SignedOut,
}

#[derive(Debug, Clone)]
pub struct SessionController {
    phase: Phase,
    organizations: Vec<Organization>,
    notice: Option<Notice>,
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionController {
    pub fn new() -> Self {
        Self {
            phase: Phase::Booting,
            organizations: Vec::new(),
            notice: None,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn organizations(&self) -> &[Organization] {
        &self.organizations
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn is_signed_in(&self) -> bool {
        matches!(
            self.phase,
            Phase::LoadingProfile
                | Phase::Unavailable { .. }
                | Phase::SelectingOrganization
                | Phase::Ready { .. }
        )
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::ServerMissing => self.reset(Phase::NeedsServer, None),
            Event::Connecting => self.reset(Phase::Booting, None),
            Event::NoSession => self.reset(Phase::SignedOut, self.notice.clone()),
            Event::SessionFound | Event::SignedIn => {
                self.notice = None;
                self.phase = Phase::LoadingProfile;
            }
            Event::SignInStarted => {
                self.notice = None;
                self.phase = Phase::SigningIn;
            }
            Event::SignInCancelled => self.reset(Phase::SignedOut, None),
            Event::SignInFailed(message) => {
                self.reset(Phase::SignedOut, Some(Notice::Message(message)))
            }
            Event::ProfileLoaded {
                organizations,
                last,
            } => {
                let active: Vec<_> = organizations.into_iter().filter(|o| o.is_active).collect();
                self.notice = None;
                let remembered =
                    last.and_then(|id| active.iter().find(|o| o.organization_id == id).cloned());
                self.phase = match (remembered, active.len()) {
                    (Some(organization), _) => Phase::Ready { organization },
                    (None, 1) => Phase::Ready {
                        organization: active[0].clone(),
                    },
                    (None, 0) => {
                        self.notice = Some(Notice::NoActiveOrganization);
                        Phase::SelectingOrganization
                    }
                    (None, _) => Phase::SelectingOrganization,
                };
                self.organizations = active;
            }
            Event::ProfileFailed(message) => self.phase = Phase::Unavailable { message },
            Event::OrganizationChosen(id) => {
                if let Some(organization) =
                    self.organizations.iter().find(|o| o.organization_id == id)
                {
                    self.phase = Phase::Ready {
                        organization: organization.clone(),
                    };
                }
            }
            Event::ChangeOrganization => {
                if matches!(self.phase, Phase::Ready { .. }) && self.organizations.len() > 1 {
                    self.phase = Phase::SelectingOrganization;
                }
            }
            Event::SessionExpired => self.reset(Phase::SignedOut, Some(Notice::SessionExpired)),
            Event::SignedOut => self.reset(Phase::SignedOut, None),
        }
    }

    fn reset(&mut self, phase: Phase, notice: Option<Notice>) {
        self.phase = phase;
        self.notice = notice;
        self.organizations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(name: &str, active: bool) -> Organization {
        Organization {
            sysid: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            is_active: active,
            organization_name: name.into(),
            organization_slug: name.to_lowercase(),
        }
    }

    fn signed_in() -> SessionController {
        let mut controller = SessionController::new();
        controller.apply(Event::NoSession);
        controller.apply(Event::SignInStarted);
        controller.apply(Event::SignedIn);
        controller
    }

    #[test]
    fn sign_in_goes_through_the_browser_and_loads_the_profile() {
        let mut controller = SessionController::new();
        assert_eq!(controller.phase(), &Phase::Booting);
        controller.apply(Event::NoSession);
        assert_eq!(controller.phase(), &Phase::SignedOut);
        controller.apply(Event::SignInStarted);
        assert_eq!(controller.phase(), &Phase::SigningIn);
        controller.apply(Event::SignedIn);
        assert_eq!(controller.phase(), &Phase::LoadingProfile);
        assert!(controller.is_signed_in());
    }

    #[test]
    fn a_single_active_organization_opens_directly() {
        let mut controller = signed_in();
        let only = org("Clínica A", true);
        controller.apply(Event::ProfileLoaded {
            organizations: vec![only.clone(), org("Antiga", false)],
            last: None,
        });
        assert_eq!(controller.phase(), &Phase::Ready { organization: only });
    }

    #[test]
    fn several_organizations_ask_unless_one_is_remembered() {
        let (a, b) = (org("A", true), org("B", true));
        let mut controller = signed_in();
        controller.apply(Event::ProfileLoaded {
            organizations: vec![a.clone(), b.clone()],
            last: None,
        });
        assert_eq!(controller.phase(), &Phase::SelectingOrganization);
        controller.apply(Event::OrganizationChosen(b.organization_id));
        assert_eq!(
            controller.phase(),
            &Phase::Ready {
                organization: b.clone()
            }
        );

        let mut remembered = signed_in();
        remembered.apply(Event::ProfileLoaded {
            organizations: vec![a.clone(), b.clone()],
            last: Some(a.organization_id),
        });
        assert_eq!(remembered.phase(), &Phase::Ready { organization: a });
    }

    #[test]
    fn remembered_organization_no_longer_active_is_ignored() {
        let gone = org("Saiu", false);
        let (a, b) = (org("A", true), org("B", true));
        let mut controller = signed_in();
        controller.apply(Event::ProfileLoaded {
            organizations: vec![gone.clone(), a, b],
            last: Some(gone.organization_id),
        });
        assert_eq!(controller.phase(), &Phase::SelectingOrganization);
    }

    #[test]
    fn no_active_organization_is_explained() {
        let mut controller = signed_in();
        controller.apply(Event::ProfileLoaded {
            organizations: vec![org("Antiga", false)],
            last: None,
        });
        assert_eq!(controller.phase(), &Phase::SelectingOrganization);
        assert_eq!(controller.notice(), Some(&Notice::NoActiveOrganization));
    }

    #[test]
    fn switching_organization_only_with_more_than_one() {
        let (a, b) = (org("A", true), org("B", true));
        let mut controller = signed_in();
        controller.apply(Event::ProfileLoaded {
            organizations: vec![a.clone()],
            last: None,
        });
        controller.apply(Event::ChangeOrganization);
        assert_eq!(
            controller.phase(),
            &Phase::Ready {
                organization: a.clone()
            }
        );

        let mut two = signed_in();
        two.apply(Event::ProfileLoaded {
            organizations: vec![a, b],
            last: None,
        });
        two.apply(Event::OrganizationChosen(
            two.organizations()[0].organization_id,
        ));
        two.apply(Event::ChangeOrganization);
        assert_eq!(two.phase(), &Phase::SelectingOrganization);
    }

    #[test]
    fn expiry_cancel_and_failure_return_to_sign_in() {
        let mut controller = signed_in();
        controller.apply(Event::SessionExpired);
        assert_eq!(controller.phase(), &Phase::SignedOut);
        assert_eq!(controller.notice(), Some(&Notice::SessionExpired));
        assert!(controller.organizations().is_empty());

        controller.apply(Event::SignInStarted);
        controller.apply(Event::SignInCancelled);
        assert_eq!(controller.phase(), &Phase::SignedOut);
        assert_eq!(controller.notice(), None);

        controller.apply(Event::SignInStarted);
        controller.apply(Event::SignInFailed("recusado".into()));
        assert_eq!(
            controller.notice(),
            Some(&Notice::Message("recusado".into()))
        );
    }

    #[test]
    fn profile_failure_keeps_the_session() {
        let mut controller = signed_in();
        controller.apply(Event::ProfileFailed("sem conexão".into()));
        assert!(controller.is_signed_in());
        controller.apply(Event::SessionFound);
        assert_eq!(controller.phase(), &Phase::LoadingProfile);
    }

    #[test]
    fn missing_server_asks_for_it() {
        let mut controller = SessionController::new();
        controller.apply(Event::ServerMissing);
        assert_eq!(controller.phase(), &Phase::NeedsServer);
    }
}
