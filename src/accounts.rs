//! One panel chip per account.
//!
//! Grok Build and Grok Bot draw from the same weekly pool on current xAI
//! plans. A work profile and a personal profile stay separate. Two logins
//! for the same email (CLI plus Grok Bot) are one chip. Product breakdowns
//! and the old Bot ledger are not added onto that chip.

use std::collections::BTreeMap;

pub const LABEL_WORK: &str = "Grok (sim sys)";
pub const LABEL_PERSONAL: &str = "Grok (pers)";

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
        _ => AccountRole::Unspecified,
    }
}

pub fn link_token(role: AccountRole) -> Option<&'static str> {
    match role {
        AccountRole::Work => Some("work"),
        AccountRole::Personal => Some("personal"),
        AccountRole::Unspecified => None,
    }
}

/// Legacy panel text that should be rewritten. Custom labels return `None`.
///
/// `build` is intentionally absent. It is a superseded caption only when the
/// account already has a known role; it is not a reason to join two accounts.
pub fn canonical_label_alias(label: &str) -> Option<&'static str> {
    match normalize_label(label).as_str() {
        "work" | "work account" | "sim sys" | "bot (work)" | "grok (sim sys)" => Some(LABEL_WORK),
        "personal" | "personal account" | "me" | "pers" | "bot (pers)" | "grok (pers)" => {
            Some(LABEL_PERSONAL)
        }
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
    sort_merged(&mut merged);
    merged
}

/// Email merge, then legacy role labels, then an explicit link.
///
/// Directory names can leave both Bot profiles unspecified. Their saved
/// captions still say which role they are. The link is resolved only after
/// those captions are applied, and `build` is not a role caption.
pub fn resolve_monitored_accounts(
    seen: Vec<SeenAccount>,
    links: &BTreeMap<String, String>,
    labels: &BTreeMap<String, String>,
) -> Vec<MergedAccount> {
    let mut merged = merge_seen(seen);
    fill_roles_from_legacy_labels(&mut merged, labels);
    apply_explicit_links(merged, links)
}

fn fill_roles_from_legacy_labels(
    accounts: &mut [MergedAccount],
    labels: &BTreeMap<String, String>,
) {
    for account in accounts.iter_mut() {
        if account.role != AccountRole::Unspecified {
            continue;
        }
        let view = AccountView {
            id: account.id.clone(),
            aliases: account.aliases.clone(),
            email: account.email.clone(),
            role: account.role,
        };
        let mut agreed: Option<AccountRole> = None;
        let mut conflict = false;
        for label in associated_labels(&view, labels) {
            let role = role_from_label(&label);
            if role == AccountRole::Unspecified {
                continue;
            }
            match agreed {
                Some(previous) if previous != role => {
                    conflict = true;
                    break;
                }
                Some(_) => {}
                None => agreed = Some(role),
            }
        }
        if conflict {
            continue;
        }
        if let Some(role) = agreed {
            account.role = role;
        }
    }
}

/// Join accounts the user has explicitly confirmed.
///
/// `links` maps an account id to a role token (`work` or `personal`, or any
/// legacy caption for that role). A link is applied only when exactly one
/// other discovered account already has that role, and only when the two
/// sides do not already have different CLI logins. Anything else stays as its
/// own chip. Labels are not link keys.
pub fn apply_explicit_links(
    mut merged: Vec<MergedAccount>,
    links: &BTreeMap<String, String>,
) -> Vec<MergedAccount> {
    for (source_id, role_token) in links {
        let role = role_from_label(role_token);
        if role == AccountRole::Unspecified || source_id.is_empty() {
            tracing::info!("explicit account link not applied: role token is not work or personal");
            continue;
        }
        let Some(source_idx) = merged
            .iter()
            .position(|account| account_has_id(account, source_id))
        else {
            tracing::info!("explicit account link not applied: linked id was not discovered");
            continue;
        };
        let targets: Vec<usize> = merged
            .iter()
            .enumerate()
            .filter(|(idx, account)| *idx != source_idx && account.role == role)
            .map(|(idx, _)| idx)
            .collect();
        if targets.len() != 1 {
            tracing::info!(
                targets = targets.len(),
                "explicit account link not applied: role does not match exactly one other profile"
            );
            continue;
        }
        let source_role = merged[source_idx].role;
        if source_role != AccountRole::Unspecified && source_role != role {
            tracing::info!(
                "explicit account link not applied: this sign-in already belongs to the other role"
            );
            continue;
        }
        let target_idx = targets[0];
        let source_cli = merged[source_idx].cli_id.clone();
        let target_cli = merged[target_idx].cli_id.clone();
        if let (Some(source_cli), Some(target_cli)) = (source_cli, target_cli)
            && source_cli != target_cli
        {
            continue;
        }
        let source = merged.remove(source_idx);
        let Some(target_idx) = merged.iter().position(|account| account.role == role) else {
            merged.push(source);
            continue;
        };
        absorb(&mut merged[target_idx], source);
        merged[target_idx].role = role;
    }
    sort_merged(&mut merged);
    merged
}

fn sort_merged(merged: &mut [MergedAccount]) {
    merged.sort_by(|a, b| {
        role_rank(a.role)
            .cmp(&role_rank(b.role))
            .then_with(|| email_key(&a.email).cmp(&email_key(&b.email)))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn account_has_id(account: &MergedAccount, id: &str) -> bool {
    account.id == id
        || account.cli_id.as_deref() == Some(id)
        || account.aliases.iter().any(|alias| alias == id)
}

fn absorb(dst: &mut MergedAccount, src: MergedAccount) {
    if dst.email.is_none() {
        dst.email = src.email.clone();
    }
    if dst
        .identity
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        dst.identity = src
            .identity
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or(src.email.clone());
    }
    if let Some(cli) = src.cli_id.clone()
        && dst.cli_id.is_none()
    {
        let previous = std::mem::replace(&mut dst.id, cli.clone());
        dst.cli_id = Some(cli);
        push_alias(dst, previous);
    }
    push_alias(dst, src.id);
    for alias in src.aliases {
        push_alias(dst, alias);
    }
    dst.role = combine_roles(dst.role, src.role);
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
        let Some(caption) = caption_for(account, labels) else {
            continue;
        };
        let current = labels
            .get(&account.id)
            .map(|value| value.trim())
            .unwrap_or("");
        if current == caption {
            continue;
        }
        let caption_is_role = default_label(account.role) == Some(caption.as_str());
        let current_is_superseded = current.is_empty()
            || canonical_label_alias(current).is_some()
            || (normalize_label(current) == "build" && caption_is_role);
        if caption_is_role && current_is_superseded {
            updates.push(LabelUpdate {
                id: account.id.clone(),
                label: caption,
            });
        }
    }
    updates
}

/// Panel caption for one account.
///
/// An intentional custom label wins. The role caption is used when the stored
/// text is missing, legacy, or the superseded word `build` on an account whose
/// role is already known. `build` by itself, on an account with no role, stays
/// `build`.
pub fn caption_for(account: &AccountView, labels: &BTreeMap<String, String>) -> Option<String> {
    let stored = associated_labels(account, labels);
    if let Some(custom) = stored.iter().find(|label| is_intentional_custom(label)) {
        return Some(custom.clone());
    }
    if let Some(canonical) = default_label(account.role) {
        return Some(canonical.to_string());
    }
    stored.into_iter().next()
}

fn associated_labels(account: &AccountView, labels: &BTreeMap<String, String>) -> Vec<String> {
    let mut keys = Vec::with_capacity(account.aliases.len() + 2);
    keys.push(account.id.clone());
    keys.extend(account.aliases.iter().cloned());
    if let Some(email) = normalized_email(account.email.as_deref()) {
        keys.push(format!("email:{email}"));
    }
    keys.into_iter()
        .filter_map(|key| lookup_label(labels, &key))
        .collect()
}

fn lookup_label(labels: &BTreeMap<String, String>, key: &str) -> Option<String> {
    labels
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn is_intentional_custom(label: &str) -> bool {
    if canonical_label_alias(label).is_some() {
        return false;
    }
    normalize_label(label) != "build"
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
                    label: "Grok (sim sys)".into(),
                },
                LabelUpdate {
                    id: "cli-pers".into(),
                    label: "Grok (pers)".into(),
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
        assert_eq!(
            main.get("cli-work").map(String::as_str),
            Some("Grok (sim sys)")
        );
        assert_eq!(main.get("kept").map(String::as_str), Some("Desk"));
        assert_eq!(
            main.get("bot-pers").map(String::as_str),
            Some("Grok (pers)")
        );
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
                    label: "Grok (sim sys)".into(),
                },
                LabelUpdate {
                    id: "cli-pers".into(),
                    label: "Grok (pers)".into(),
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
        assert_eq!(updates[0].label, "Grok (sim sys)");
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
        assert_eq!(role_from_label("Grok (sim sys)"), AccountRole::Work);
        assert_eq!(role_from_label("Grok (pers)"), AccountRole::Personal);
        assert_eq!(role_from_label("Desk"), AccountRole::Unspecified);
        assert_eq!(role_from_label("build"), AccountRole::Unspecified);
        assert_eq!(role_from_label("bot (work)"), AccountRole::Work);
    }

    fn cli_without_email(id: &str) -> SeenAccount {
        SeenAccount {
            id: id.into(),
            email: None,
            identity: None,
            role: AccountRole::Unspecified,
            kind: SourceKind::Cli,
        }
    }

    fn links(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(id, role)| ((*id).to_string(), (*role).to_string()))
            .collect()
    }

    fn work_pers_and_build() -> Vec<SeenAccount> {
        vec![
            cli_without_email("cli-build"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
        ]
    }

    #[test]
    fn missing_email_build_stays_separate_until_explicitly_linked() {
        let unlinked = merge_seen(work_pers_and_build());
        assert_eq!(unlinked.len(), 3);
        let linked = apply_explicit_links(unlinked, &links(&[("cli-build", "work")]));
        assert_eq!(linked.len(), 2);
        let work = linked
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        let pers = linked
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        assert_eq!(work.cli_id.as_deref(), Some("cli-build"));
        assert!(work.aliases.iter().any(|id| id == "bot-work"));
        assert!(pers.cli_id.is_none());
        assert_eq!(pers.id, "bot-pers");
    }

    #[test]
    fn different_email_still_needs_an_explicit_link() {
        let seen = vec![
            cli("cli-build", "build-login@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
        ];
        assert_eq!(merge_seen(seen.clone()).len(), 3);
        let linked = apply_explicit_links(merge_seen(seen), &links(&[("cli-build", "work")]));
        assert_eq!(linked.len(), 2);
        assert!(linked.iter().any(|account| {
            account.cli_id.as_deref() == Some("cli-build") && account.role == AccountRole::Work
        }));
    }

    #[test]
    fn labels_do_not_join_build_to_a_bot_account() {
        let linked = apply_explicit_links(merge_seen(work_pers_and_build()), &BTreeMap::new());
        assert_eq!(linked.len(), 3);
        let labels = BTreeMap::from([
            ("cli-build".into(), "build".into()),
            ("bot-work".into(), "bot (work)".into()),
            ("bot-pers".into(), "bot (pers)".into()),
        ]);
        let captions: Vec<_> = linked
            .iter()
            .map(|account| caption_for(&view(account), &labels))
            .collect();
        assert!(
            captions
                .iter()
                .any(|caption| caption.as_deref() == Some("build"))
        );
        assert!(
            captions
                .iter()
                .any(|caption| caption.as_deref() == Some("Grok (sim sys)"))
        );
        assert!(
            captions
                .iter()
                .any(|caption| caption.as_deref() == Some("Grok (pers)"))
        );
    }

    #[test]
    fn explicit_link_makes_two_captions_and_does_not_sum_usage() {
        let linked = apply_explicit_links(
            merge_seen(work_pers_and_build()),
            &links(&[("cli-build", "work")]),
        );
        assert_eq!(linked.len(), 2);
        let work = linked
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        let pers = linked
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        let labels = BTreeMap::from([
            ("cli-build".into(), "build".into()),
            ("bot-work".into(), "bot (work)".into()),
            ("bot-pers".into(), "bot (pers)".into()),
        ]);
        assert_eq!(
            caption_for(&view(work), &labels).as_deref(),
            Some("Grok (sim sys)")
        );
        assert_eq!(
            caption_for(&view(pers), &labels).as_deref(),
            Some("Grok (pers)")
        );
        let readings = [
            UsageReading {
                bearer_id: "cli-build".into(),
                percent: 10.0,
            },
            UsageReading {
                bearer_id: "bot-work".into(),
                percent: 99.0,
            },
            UsageReading {
                bearer_id: "bot-pers".into(),
                percent: 40.0,
            },
        ];
        assert_eq!(assigned_percent(work, &readings), Some(10.0));
        assert_eq!(assigned_percent(pers, &readings), None);
        assert_ne!(assigned_percent(work, &readings), Some(109.0));
        assert_ne!(assigned_percent(work, &readings), Some(50.0));
    }

    #[test]
    fn unmapped_third_login_is_preserved() {
        let seen = vec![
            cli_without_email("cli-build"),
            cli("cli-extra", "extra@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
        ];
        let linked = apply_explicit_links(merge_seen(seen), &links(&[("cli-build", "work")]));
        assert_eq!(linked.len(), 3);
        assert!(linked.iter().any(|account| {
            account.cli_id.as_deref() == Some("cli-extra")
                && account.role == AccountRole::Unspecified
        }));
        let labels = BTreeMap::from([("cli-extra".into(), "build".into())]);
        let extra = linked
            .iter()
            .find(|account| account.cli_id.as_deref() == Some("cli-extra"))
            .unwrap();
        assert_eq!(caption_for(&view(extra), &labels).as_deref(), Some("build"));
    }

    #[test]
    fn email_match_is_not_pulled_onto_the_other_role() {
        let seen = vec![
            cli("cli-build", "pers@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
            bot("bot-pers", "pers@example.com", "Grok Bot"),
        ];
        let linked = apply_explicit_links(merge_seen(seen), &links(&[("cli-build", "work")]));
        assert_eq!(linked.len(), 2);
        let pers = linked
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        assert_eq!(pers.cli_id.as_deref(), Some("cli-build"));
        assert!(
            linked
                .iter()
                .find(|account| account.role == AccountRole::Work)
                .unwrap()
                .cli_id
                .is_none()
        );
    }

    #[test]
    fn two_work_profiles_block_the_link() {
        let seen = vec![
            cli_without_email("cli-build"),
            bot("bot-a", "a@example.com", "Grok Bot Work"),
            bot("bot-b", "b@example.com", "Grok Bot Work"),
        ];
        let linked = apply_explicit_links(merge_seen(seen), &links(&[("cli-build", "work")]));
        assert_eq!(linked.len(), 3);
        assert!(linked.iter().any(|account| {
            account.id == "cli-build" && account.cli_id.as_deref() == Some("cli-build")
        }));
    }

    #[test]
    fn second_cli_link_does_not_replace_the_first_login() {
        let seen = vec![
            cli("cli-a", "a-login@example.com"),
            cli("cli-b", "b-login@example.com"),
            bot("bot-work", "work@example.com", "Grok Bot Work"),
        ];
        let linked = apply_explicit_links(
            merge_seen(seen),
            &links(&[("cli-a", "work"), ("cli-b", "work")]),
        );
        assert_eq!(linked.len(), 2);
        let work = linked
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        let other = linked
            .iter()
            .find(|account| account.role != AccountRole::Work)
            .unwrap();
        assert!(work.cli_id.is_some());
        assert_ne!(work.cli_id, other.cli_id);
        let readings = [
            UsageReading {
                bearer_id: work.cli_id.clone().unwrap(),
                percent: 10.0,
            },
            UsageReading {
                bearer_id: other.cli_id.clone().unwrap(),
                percent: 70.0,
            },
        ];
        assert_eq!(assigned_percent(work, &readings), Some(10.0));
        assert_eq!(assigned_percent(other, &readings), Some(70.0));
    }

    #[test]
    fn legacy_captions_migrate_and_custom_text_stays() {
        let mut labels = BTreeMap::from([
            ("work-id".into(), "sim sys".into()),
            ("pers-id".into(), "bot (pers)".into()),
            ("old-work".into(), "bot (work)".into()),
            ("custom".into(), "Desk".into()),
            ("build-id".into(), "build".into()),
        ]);
        assert!(migrate_label_values(&mut labels));
        assert_eq!(labels.get("work-id").map(String::as_str), Some(LABEL_WORK));
        assert_eq!(
            labels.get("pers-id").map(String::as_str),
            Some(LABEL_PERSONAL)
        );
        assert_eq!(labels.get("old-work").map(String::as_str), Some(LABEL_WORK));
        assert_eq!(labels.get("custom").map(String::as_str), Some("Desk"));
        assert_eq!(labels.get("build-id").map(String::as_str), Some("build"));
    }

    #[test]
    fn desk_label_is_not_replaced_by_the_role_caption() {
        let account = AccountView {
            id: "cli-work".into(),
            aliases: vec!["bot-work".into()],
            email: Some("work@example.com".into()),
            role: AccountRole::Work,
        };
        let labels = BTreeMap::from([("bot-work".into(), "Desk".into())]);
        assert_eq!(caption_for(&account, &labels).as_deref(), Some("Desk"));
        assert!(label_updates(&labels, &[account]).is_empty());
    }

    fn unspecified(id: &str, email: Option<&str>, kind: SourceKind) -> SeenAccount {
        SeenAccount {
            id: id.into(),
            email: email.map(str::to_string),
            identity: email.map(str::to_string),
            role: AccountRole::Unspecified,
            kind,
        }
    }

    /// The deployed failure: captions already migrated, directory roles absent,
    /// and the Build key explicitly linked to work. The helper-only tests never
    /// loaded those captions before resolving the link.
    #[test]
    fn migrated_captions_let_an_explicit_work_link_join_a_login_without_email() {
        let build_id = "https://auth.x.ai::example-build";
        let seen = vec![
            unspecified(build_id, None, SourceKind::Cli),
            unspecified("bot-work", Some("work@example.com"), SourceKind::Bot),
            unspecified("bot-pers", Some("pers@example.com"), SourceKind::Bot),
        ];
        let labels = BTreeMap::from([
            (build_id.into(), "build".into()),
            ("bot-work".into(), "Grok (sim sys)".into()),
            ("bot-pers".into(), "Grok (pers)".into()),
        ]);
        let links = BTreeMap::from([(build_id.into(), "work".into())]);

        let unresolved = apply_explicit_links(merge_seen(seen.clone()), &links);
        assert_eq!(
            unresolved.len(),
            3,
            "directory roles are unset, so the link alone cannot see a work profile"
        );

        let resolved = resolve_monitored_accounts(seen, &links, &labels);
        assert_eq!(resolved.len(), 2);
        let work = resolved
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        let pers = resolved
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        assert_eq!(work.cli_id.as_deref(), Some(build_id));
        assert!(work.aliases.iter().any(|id| id == "bot-work"));
        assert!(pers.cli_id.is_none());
        assert_eq!(
            caption_for(&view(work), &labels).as_deref(),
            Some("Grok (sim sys)")
        );
        assert_eq!(
            caption_for(&view(pers), &labels).as_deref(),
            Some("Grok (pers)")
        );
        let readings = [
            UsageReading {
                bearer_id: build_id.into(),
                percent: 12.0,
            },
            UsageReading {
                bearer_id: "bot-work".into(),
                percent: 80.0,
            },
            UsageReading {
                bearer_id: "bot-pers".into(),
                percent: 40.0,
            },
        ];
        assert_eq!(assigned_percent(work, &readings), Some(12.0));
        assert_eq!(assigned_percent(pers, &readings), None);
        assert_ne!(assigned_percent(work, &readings), Some(92.0));
    }

    #[test]
    fn explicit_link_without_a_role_caption_keeps_the_build_chip() {
        let seen = vec![
            unspecified("cli-build", None, SourceKind::Cli),
            unspecified("bot-a", Some("a@example.com"), SourceKind::Bot),
            unspecified("bot-b", Some("b@example.com"), SourceKind::Bot),
        ];
        let labels = BTreeMap::from([
            ("cli-build".into(), "build".into()),
            ("bot-a".into(), "Desk".into()),
            ("bot-b".into(), "Other".into()),
        ]);
        let links = BTreeMap::from([("cli-build".into(), "work".into())]);
        let resolved = resolve_monitored_accounts(seen, &links, &labels);
        assert_eq!(resolved.len(), 3);
        assert!(
            resolved
                .iter()
                .any(|account| account.cli_id.as_deref() == Some("cli-build"))
        );
        assert_eq!(
            caption_for(
                &view(
                    resolved
                        .iter()
                        .find(|account| account.cli_id.as_deref() == Some("cli-build"))
                        .unwrap()
                ),
                &labels
            )
            .as_deref(),
            Some("build")
        );
    }

    #[test]
    fn label_role_does_not_pull_an_email_matched_login_off_personal() {
        let seen = vec![
            unspecified("cli-build", Some("pers@example.com"), SourceKind::Cli),
            unspecified("bot-work", Some("work@example.com"), SourceKind::Bot),
            unspecified("bot-pers", Some("pers@example.com"), SourceKind::Bot),
        ];
        let labels = BTreeMap::from([
            ("cli-build".into(), "build".into()),
            ("bot-work".into(), "bot (work)".into()),
            ("bot-pers".into(), "bot (pers)".into()),
        ]);
        let links = BTreeMap::from([("cli-build".into(), "work".into())]);
        let resolved = resolve_monitored_accounts(seen, &links, &labels);
        assert_eq!(resolved.len(), 2);
        let pers = resolved
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        assert_eq!(pers.cli_id.as_deref(), Some("cli-build"));
        assert!(
            resolved
                .iter()
                .find(|account| account.role == AccountRole::Work)
                .unwrap()
                .cli_id
                .is_none()
        );
    }

    #[test]
    fn a_second_cli_login_can_join_personal_without_moving_work() {
        let seen = vec![
            unspecified("https://auth.x.ai::work", None, SourceKind::Cli),
            unspecified("https://auth.x.ai::pers", None, SourceKind::Cli),
            unspecified("bot-work", Some("work@example.com"), SourceKind::Bot),
            unspecified("bot-pers", Some("pers@example.com"), SourceKind::Bot),
        ];
        let labels = BTreeMap::from([
            ("https://auth.x.ai::work".into(), "build".into()),
            ("bot-work".into(), "Grok (sim sys)".into()),
            ("bot-pers".into(), "Grok (pers)".into()),
        ]);
        let links = BTreeMap::from([
            ("https://auth.x.ai::work".into(), "work".into()),
            ("https://auth.x.ai::pers".into(), "personal".into()),
        ]);
        let resolved = resolve_monitored_accounts(seen, &links, &labels);
        assert_eq!(resolved.len(), 2);
        let work = resolved
            .iter()
            .find(|account| account.role == AccountRole::Work)
            .unwrap();
        let pers = resolved
            .iter()
            .find(|account| account.role == AccountRole::Personal)
            .unwrap();
        assert_eq!(work.cli_id.as_deref(), Some("https://auth.x.ai::work"));
        assert_eq!(pers.cli_id.as_deref(), Some("https://auth.x.ai::pers"));
        let readings = [
            UsageReading {
                bearer_id: "https://auth.x.ai::work".into(),
                percent: 10.0,
            },
            UsageReading {
                bearer_id: "https://auth.x.ai::pers".into(),
                percent: 55.0,
            },
        ];
        assert_eq!(assigned_percent(work, &readings), Some(10.0));
        assert_eq!(assigned_percent(pers, &readings), Some(55.0));
        assert_ne!(assigned_percent(work, &readings), Some(65.0));
    }
}
