use super::*;

fn plan_for_action(plan: &InstallPlan, action: &InstallStepAction) -> InstallPlan {
    let mut scoped = plan.clone();
    scoped
        .steps
        .retain(|step| step.action.as_ref() == Some(action));
    scoped
}

fn receipt_for_action(plan: &InstallPlan, action: &InstallStepAction) -> Option<InstallReceipt> {
    receipt_for_plan(&plan_for_action(plan, action))
}

pub fn receipt_match_details(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<(bool, Option<ReceiptOwnershipDigest>), String> {
    let Some(expected) = receipt_for_plan(plan) else {
        return Ok((false, None));
    };
    let Some(action_expected) = receipt_for_action(plan, action) else {
        return Ok((false, None));
    };
    let receipt_path = receipt_path_for_plan(plan);
    validate_receipt_location(plan, &receipt_path)?;
    for actual in read_receipts_for_action(plan, action)? {
        if !same_identity(&actual, &expected) && !same_identity(&actual, &action_expected) {
            continue;
        }
        return match action {
            InstallStepAction::PipInstall { .. } => Ok((
                pip_receipt_is_healthy_for_action(&action_expected, action)?,
                None,
            )),
            InstallStepAction::GitClone { ref_, dest, .. } => {
                if !git_checkout_is_healthy(dest, ref_.as_deref()) {
                    return Ok((false, None));
                }
                let digest = receipt_ownership_digest_for_action(&actual, action)?;
                Ok((digest.is_some(), digest))
            }
            InstallStepAction::ZipExtract { .. }
            | InstallStepAction::PathCopy { .. }
            | InstallStepAction::AdobePluginLink { .. } => {
                let digest = receipt_ownership_digest_for_action(&actual, action)?;
                Ok((digest.is_some(), digest))
            }
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => Ok((false, None)),
        };
    }
    Ok((false, None))
}

fn read_receipts_for_action(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<Vec<InstallReceipt>, String> {
    let scoped = plan_for_action(plan, action);
    let paths = [
        Some(receipt_path_for_plan(plan)),
        Some(receipt_path_for_plan(&scoped)),
        legacy_receipt_path_for_plan(plan),
        legacy_receipt_path_for_plan(&scoped),
        legacy_non_pip_receipt_path_for_plan(plan),
        legacy_non_pip_receipt_path_for_plan(&scoped),
    ];
    let mut receipts = Vec::new();
    let mut seen_paths = Vec::new();
    for path in paths.into_iter().flatten() {
        if seen_paths.iter().any(|seen| seen == &path) {
            continue;
        }
        seen_paths.push(path.clone());
        if let Some(receipt) = read_receipt(&path)? {
            receipts.push(receipt);
        }
    }
    Ok(receipts)
}
