//! One panel chip per account.
//!
//! Grok Build and Grok Bot draw from the same weekly pool on current xAI
//! plans. A work profile and a personal profile stay separate. Two logins
//! for the same email (CLI plus Grok Bot) are one chip. Product breakdowns
//! and the old Bot ledger are not added onto that chip.

use std::collections::BTreeMap;

pub const LABEL_WORK: &str = "sim sys";
pub const LABEL_PERSONAL: &str = "pers";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccountRole {
    Work,
    Personal,
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Cli,
    Bot,
}

#[derive(Debug, Clone)]
pub struct SeenAccount {
    pub id: String,
    pub email: Option<String>,
    pub identity: Option<String>,
    pub role: AccountRole,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedAccount {
    pub id: String,
    pub aliases: Vec<String>,
    pub email: Option<String>,
    pub identity: Option<String>,
    pub role: AccountRole,
    pub cli_id: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub bearer_id: String,
    pub percent: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub id: String,
    pub aliases: Vec<String>,
    pub email: Option<String>,
    pub role: AccountRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelUpdate {
    pub id: String,
    pub label: String,
}

pub fn role_from_dir_name(name: &str) -> AccountRole {
    let trimmed = name.trim();
    if trimmed.eq_ignore_ascii_case("Grok Bot Work")
        || trimmed.to_ascii_lowercase().ends_with(" work")
    {
        AccountRole::Work
    } else if trimmed.eq_ignore_ascii_case("Grok Bot") {
        AccountRole::Personal
    } else {
        AccountRole::Unspecified
    }
}

pub fn role_from_label(label: &str) -> AccountRole {
    match canonical_label_alias(label) {
        Some(LABEL_WORK) => AccountRole::Work,
        Some(LABEL_PERSONAL) => AccountRole::Personal,
        _ => match normalize_label(label).as_str() {
            "sim sys" => AccountRole::Work,
            "pers" => AccountRole::Personal,
            _ => AccountRole::Unspecified,
        },
    }
}

/// Legacy panel text that should be rewritten. Custom labels return `None`.
pub fn canonical_label_alias(label: &str) -> Option<&'static str> {
    match normalize_label(label).as_str() {
        "work" | "work account" => Some(LABEL_WORK),
        "personal" | "personal account" | "me" => Some(LABEL_PERSONAL),
        _ => None,
    }
}

pub fn merge_seen(seen: Vec<SeenAccount>) -> Vec<MergedAccount> {
    let mut groups: Vec<(String, MergedAccount)> = Vec::new();
    for item in seen {
        let key = merge_key(&item);
        if let Some((_, existing)) = groups.iter_mut().find(|(k, _)| k == &key) {
            fold_into(existing, item);
        } else {
            groups.push((key, merged_from(item)));
        }
    }
    let mut merged: Vec<_> = groups.into_iter().map(|(_, account)| account).collect();
    merged.sort_by(|a, b| {
        role_rank(a.role)
            .cmp(&role_rank(b.role))
            .then_with(|| email_key(&a.email).cmp(&email_key(&b.email)))
            .then_with(|| a.id.cmp(&b.id))
    });
    merged
}

/// The chip shows the shared-pool reading for its own CLI login.
/// A second reading for that login is not added. Another account's reading
/// is not used. The old Bot ledger is not a bearer id, so it cannot land here.
#[cfg(test)]
pub fn assigned_percent(account: &MergedAccount, readings: &[UsageReading]) -> Option<f32> {
    let cli_id = account.cli_id.as_deref()?;
    readings
        .iter()
        .find(|reading| reading.bearer_id == cli_id)
        .map(|reading| reading.percent)
}

pub fn import_and_migrate_labels(
    dest: &mut BTreeMap<String, String>,
    src: &BTreeMap<String, String>,
) -> bool {
    let before = dest.clone();
    for (key, value) in src {
        if value.trim().is_empty() {
            continue;
        }
        dest.entry(key.clone()).or_insert_with(|| value.clone());
    }
    migrate_label_values(dest);
    dest != &before
}

pub fn migrate_label_values(labels: &mut BTreeMap<String, String>) -> bool {
    let mut changed = false;
    let keys: Vec<String> = labels.keys().cloned().collect();
    for key in keys {
        let Some(value) = labels.get(&key).cloned() else {
            continue;
        };
        if let Some(next) = canonical_label_alias(&value)
            && next != value
        {
            labels.insert(key, next.to_string());
            changed = true;
        }
    }
    changed
}

pub fn label_updates(
    labels: &BTreeMap<String, String>,
    accounts: &[AccountView],
) -> Vec<LabelUpdate> {
    let mut accounts = accounts.to_vec();
    finalize_roles(&mut accounts, labels);
    let mut updates = Vec::new();
    for account in &accounts {
        if account.id.is_empty() {
            continue;
        }
        if let Some(stored) = stored_label(labels, account) {
            if let Some(next) = canonical_label_alias(&stored)
                && stored != next
            {
                updates.push(LabelUpdate {
                    id: account.id.clone(),
                    label: next.to_string(),
                });
            }
            continue;
        }
        let Some(label) = default_label(account.role) else {
            continue;
        };
        updates.push(LabelUpdate {
            id: account.id.clone(),
            label: label.to_string(),
        });
    }
    updates
}

fn finalize_roles(accounts: &mut [AccountView], labels: &BTreeMap<String, String>) {
    for account in accounts.iter_mut() {
        if account.role != AccountRole::Unspecified {
            continue;
        }
        if let Some(label) = stored_label(labels, account) {
            let inferred = role_from_label(&label);
            if inferred != AccountRole::Unspecified {
                account.role = inferred;
            }
        }
    }
    if accounts.len() != 2 {
        return;
    }
    match (accounts[0].role, accounts[1].role) {
        (AccountRole::Work, AccountRole::Unspecified) => accounts[1].role = AccountRole::Personal,
        (AccountRole::Unspecified, AccountRole::Work) => accounts[0].role = AccountRole::Personal,
        (AccountRole::Personal, AccountRole::Unspecified) => accounts[1].role = AccountRole::Work,
        (AccountRole::Unspecified, AccountRole::Personal) => accounts[0].role = AccountRole::Work,
        _ => {}
    }
}

fn stored_label(labels: &BTreeMap<String, String>, account: &AccountView) -> Option<String> {
    let mut keys = Vec::with_capacity(account.aliases.len() + 2);
    keys.push(account.id.clone());
    keys.extend(account.aliases.iter().cloned());
    if let Some(email) = normalized_email(account.email.as_deref()) {
        keys.push(format!("email:{email}"));
    }
    keys.into_iter().find_map(|key| {
        labels
            .get(&key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn default_label(role: AccountRole) -> Option<&'static str> {
    match role {
        AccountRole::Work => Some(LABEL_WORK),
        AccountRole::Personal => Some(LABEL_PERSONAL),
        AccountRole::Unspecified => None,
    }
}

fn merged_from(item: SeenAccount) -> MergedAccount {
    let cli_id = matches!(item.kind, SourceKind::Cli).then(|| item.id.clone());
    let identity = item
        .identity
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item.email.clone());
    MergedAccount {
        id: item.id,
        aliases: Vec::new(),
        email: item.email,
        identity,
        role: item.role,
        cli_id,
    }
}

fn fold_into(dst: &mut MergedAccount, item: SeenAccount) {
    if dst.email.is_none() {
        dst.email = item.email.clone();
    }
    if dst
        .identity
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        dst.identity = item
            .identity
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| item.email.clone());
    }
    dst.role = combine_roles(dst.role, item.role);
    match item.kind {
        SourceKind::Cli => {
            if dst.cli_id.is_none() {
                let previous = std::mem::replace(&mut dst.id, item.id.clone());
                dst.cli_id = Some(item.id);
                push_alias(dst, previous);
            } else {
                push_alias(dst, item.id);
            }
        }
        SourceKind::Bot => push_alias(dst, item.id),
    }
}

fn push_alias(dst: &mut MergedAccount, id: String) {
    if id.is_empty() || id == dst.id || dst.aliases.iter().any(|existing| existing == &id) {
        return;
    }
    if dst.cli_id.as_ref().is_some_and(|cli| cli == &id) {
        return;
    }
    dst.aliases.push(id);
}

fn combine_roles(left: AccountRole, right: AccountRole) -> AccountRole {
    match (left, right) {
        (AccountRole::Work, AccountRole::Work) => AccountRole::Work,
        (AccountRole::Personal, AccountRole::Personal) => AccountRole::Personal,
        (AccountRole::Unspecified, other) | (other, AccountRole::Unspecified) => other,
        _ => AccountRole::Unspecified,
    }
}

fn merge_key(item: &SeenAccount) -> String {
    normalized_email(item.email.as_deref())
        .map(|email| format!("email:{email}"))
        .unwrap_or_else(|| format!("id:{}", item.id.to_ascii_lowercase()))
}

fn normalized_email(email: Option<&str>) -> Option<String> {
    let email = email?.trim();
    if email.is_empty() || !email.contains('@') {
        return None;
    }
    Some(email.to_ascii_lowercase())
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn email_key(email: &Option<String>) -> String {
    normalized_email(email.as_deref()).unwrap_or_default()
}

fn role_rank(role: AccountRole) -> u8 {
    match role {
        AccountRole::Work => 0,
        AccountRole::Personal => 1,
        AccountRole::Unspecified => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(id: &str, email: &str) -> SeenAccount {
        SeenAccount {
            id: id.into(),
            email: Some(email.into()),
            identity: Some(email.into()),
            role: AccountRole::Unspecified,
            kind: SourceKind::Cli,
        }
    }

    fn bot(id: &str, email: &str, dir: &str) -> SeenAccount {
        SeenAccount {
            id: id.into(),
            email: Some(email.into()),
            identity: Some(email.into()),
            role: role_from_dir_name(dir),
            kind: SourceKind::Bot,
        }
    }

    fn view(account: &MergedAccount) -> AccountView {
        AccountView {
            id: account.id.clone(),
            aliases: account.aliases.clone(),
            email: account.email.clone(),
            role: account.role,
        }
    }

    #[test]
    fn work_and_personal_stay_distinct_and_get_exact_labels() {
        let merged = merge_seen(vec![
            cli("cli-pers", "pers@example.com"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
            cli("cli-work", "work@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].email.as_deref(), Some("work@example.com"));
        assert_eq!(merged[0].role, AccountRole::Work);
        assert_eq!(merged[0].cli_id.as_deref(), Some("cli-work"));
        assert!(merged[0].aliases.iter().any(|id| id == "bot-work"));
        assert_eq!(merged[1].email.as_deref(), Some("pers@example.com"));
        assert_eq!(merged[1].role, AccountRole::Personal);
        assert_eq!(merged[1].id, "cli-pers");

        let views: Vec<_> = merged.iter().map(view).collect();
        let updates = label_updates(&BTreeMap::new(), &views);
        assert_eq!(
            updates,
            vec![
                LabelUpdate {
                    id: "cli-work".into(),
                    label: "sim sys".into(),
                },
                LabelUpdate {
                    id: "cli-pers".into(),
                    label: "pers".into(),
                },
            ]
        );
    }

    #[test]
    fn same_email_from_build_and_bot_is_one_account() {
        let merged = merge_seen(vec![
            bot("bot-1", "Work@Example.com", "Grok Bot Work"),
            cli("cli-1", "work@example.com"),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "cli-1");
        assert_eq!(merged[0].role, AccountRole::Work);
        assert_eq!(merged[0].aliases, vec!["bot-1".to_string()]);
        assert_eq!(merged[0].cli_id.as_deref(), Some("cli-1"));
    }

    #[test]
    fn usage_is_not_summed_across_accounts_or_bot_ledger() {
        let merged = merge_seen(vec![
            cli("cli-work", "work@example.com"),
            cli("cli-pers", "pers@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
        ]);
        let readings = vec![
            UsageReading {
                bearer_id: "cli-work".into(),
                percent: 10.0,
            },
            UsageReading {
                bearer_id: "cli-pers".into(),
                percent: 40.0,
            },
            UsageReading {
                bearer_id: "bot-work".into(),
                percent: 99.0,
            },
        ];
        let work = &merged[0];
        let pers = &merged[1];
        assert_eq!(assigned_percent(work, &readings), Some(10.0));
        assert_eq!(assigned_percent(pers, &readings), Some(40.0));
        assert_ne!(assigned_percent(work, &readings), Some(50.0));
        assert_ne!(assigned_percent(work, &readings), Some(109.0));
    }

    #[test]
    fn duplicate_readings_for_one_login_are_not_added() {
        let merged = merge_seen(vec![cli("cli-work", "work@example.com")]);
        let readings = vec![
            UsageReading {
                bearer_id: "cli-work".into(),
                percent: 10.0,
            },
            UsageReading {
                bearer_id: "cli-work".into(),
                percent: 15.0,
            },
        ];
        assert_eq!(assigned_percent(&merged[0], &readings), Some(10.0));
    }

    #[test]
    fn bot_only_account_has_no_borrowed_percent() {
        let merged = merge_seen(vec![
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            cli("cli-pers", "pers@example.com"),
        ]);
        assert_eq!(merged.len(), 2);
        let readings = [UsageReading {
            bearer_id: "cli-pers".into(),
            percent: 40.0,
        }];
        let work = merged
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        assert!(work.cli_id.is_none());
        assert_eq!(assigned_percent(work, &readings), None);
        assert_eq!(
            assigned_percent(
                merged
                    .iter()
                    .find(|account| account.cli_id.as_deref() == Some("cli-pers"))
                    .unwrap(),
                &readings
            ),
            Some(40.0)
        );
    }

    #[test]
    fn migrates_work_and_personal_labels_without_clobbering_custom() {
        let mut main = BTreeMap::from([
            ("cli-work".into(), "Work".into()),
            ("kept".into(), "Desk".into()),
        ]);
        let bot = BTreeMap::from([
            ("cli-work".into(), "personal".into()),
            ("bot-pers".into(), "personal account".into()),
            ("empty".into(), "  ".into()),
        ]);
        assert!(import_and_migrate_labels(&mut main, &bot));
        assert_eq!(main.get("cli-work").map(String::as_str), Some("sim sys"));
        assert_eq!(main.get("kept").map(String::as_str), Some("Desk"));
        assert_eq!(main.get("bot-pers").map(String::as_str), Some("pers"));
        assert!(!main.contains_key("empty"));
        assert!(!import_and_migrate_labels(&mut main, &bot));
    }

    #[test]
    fn unlabeled_partner_of_work_becomes_pers() {
        let accounts = vec![
            AccountView {
                id: "cli-work".into(),
                aliases: Vec::new(),
                email: Some("work@example.com".into()),
                role: AccountRole::Unspecified,
            },
            AccountView {
                id: "cli-pers".into(),
                aliases: Vec::new(),
                email: Some("pers@example.com".into()),
                role: AccountRole::Unspecified,
            },
        ];
        let labels = BTreeMap::from([("cli-work".into(), "work".into())]);
        let updates = label_updates(&labels, &accounts);
        assert_eq!(
            updates,
            vec![
                LabelUpdate {
                    id: "cli-work".into(),
                    label: "sim sys".into(),
                },
                LabelUpdate {
                    id: "cli-pers".into(),
                    label: "pers".into(),
                },
            ]
        );
    }

    #[test]
    fn three_accounts_are_not_guessed() {
        let accounts = vec![
            AccountView {
                id: "a".into(),
                aliases: Vec::new(),
                email: Some("a@example.com".into()),
                role: AccountRole::Work,
            },
            AccountView {
                id: "b".into(),
                aliases: Vec::new(),
                email: Some("b@example.com".into()),
                role: AccountRole::Unspecified,
            },
            AccountView {
                id: "c".into(),
                aliases: Vec::new(),
                email: Some("c@example.com".into()),
                role: AccountRole::Unspecified,
            },
        ];
        let updates = label_updates(&BTreeMap::new(), &accounts);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].label, "sim sys");
        assert_eq!(updates[0].id, "a");
    }

    #[test]
    fn custom_label_is_kept() {
        let accounts = [AccountView {
            id: "cli-work".into(),
            aliases: vec!["bot-work".into()],
            email: Some("work@example.com".into()),
            role: AccountRole::Work,
        }];
        let labels = BTreeMap::from([("bot-work".into(), "Desk".into())]);
        assert!(label_updates(&labels, &accounts).is_empty());
    }

    #[test]
    fn dir_names_mark_work_and_personal() {
        assert_eq!(role_from_dir_name("Grok Bot Work"), AccountRole::Work);
        assert_eq!(role_from_dir_name("grok bot work"), AccountRole::Work);
        assert_eq!(role_from_dir_name("Grok Bot"), AccountRole::Personal);
        assert_eq!(
            role_from_dir_name("Grok Bot Extra"),
            AccountRole::Unspecified
        );
        assert_eq!(role_from_label("Work"), AccountRole::Work);
        assert_eq!(role_from_label("sim sys"), AccountRole::Work);
        assert_eq!(role_from_label("Personal"), AccountRole::Personal);
        assert_eq!(role_from_label("pers"), AccountRole::Personal);
        assert_eq!(role_from_label("Desk"), AccountRole::Unspecified);
    }
}
