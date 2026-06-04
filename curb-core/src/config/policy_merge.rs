use std::collections::HashSet;

use super::{Agent, AgentKind, Config, Policy, defaults::default_process_agents};

pub(super) fn policy_for(config: &Config, agent: &Agent) -> Policy {
    let mut policy = config.defaults.clone();
    if let Some(override_policy) = &agent.policy {
        policy.merge_override(override_policy);
    }
    policy
}

pub(super) fn refresh_agent_policies(config: &mut Config) {
    let previous_defaults = config.defaults.clone();
    refresh_agent_policies_from_previous_defaults(config, &previous_defaults);
}

pub(super) fn refresh_agent_policies_from_previous_defaults(
    config: &mut Config,
    previous_defaults: &Policy,
) {
    let defaults = config.defaults.clone();
    for agent in &mut config.agents {
        let mut policy = defaults.clone();
        if let Some(override_policy) = &agent.policy {
            let override_policy = policy_delta(previous_defaults, override_policy);
            policy.merge_override(&override_policy);
        }
        policy.allow_app_root_kill = false;
        agent.policy = Some(policy);
    }
}

fn policy_delta(previous_defaults: &Policy, existing: &Policy) -> Policy {
    let mut effective = previous_defaults.clone();
    effective.merge_override(existing);

    let mut delta = Policy::default();
    if effective.warn_after != previous_defaults.warn_after {
        delta.warn_after = effective.warn_after;
    }
    if effective.kill_after != previous_defaults.kill_after {
        delta.kill_after = effective.kill_after;
    }
    if effective.ack_extension != previous_defaults.ack_extension {
        delta.ack_extension = effective.ack_extension;
    }
    if effective.max_extensions != previous_defaults.max_extensions {
        delta.max_extensions = effective.max_extensions;
    }
    if effective.kill_grace_period != previous_defaults.kill_grace_period {
        delta.kill_grace_period = effective.kill_grace_period;
    }
    if effective.cooldown_after_kill != previous_defaults.cooldown_after_kill {
        delta.cooldown_after_kill = effective.cooldown_after_kill;
    }
    if effective.min_lifetime != previous_defaults.min_lifetime {
        delta.min_lifetime = effective.min_lifetime;
    }
    if effective.max_run_gap != previous_defaults.max_run_gap {
        delta.max_run_gap = effective.max_run_gap;
    }
    if effective.allow_app_root_kill && !previous_defaults.allow_app_root_kill {
        delta.allow_app_root_kill = true;
    }
    delta
}

pub(super) fn keep_process_agents(config: &mut Config) {
    let mut agents = default_process_agents();
    let mut seen = agents
        .iter()
        .map(|agent| agent.id.clone())
        .collect::<HashSet<_>>();
    for agent in &config.agents {
        if seen.contains(&agent.id) || !agent.termination_allowed() {
            continue;
        }
        let mut agent = agent.clone();
        if agent.kind == AgentKind::Unspecified {
            agent.kind = AgentKind::Process;
        }
        seen.insert(agent.id.clone());
        agents.push(agent);
    }
    config.agents = agents;
}
