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
    for agent in &mut config.agents {
        let mut policy = config.defaults.clone();
        policy.allow_app_root_kill = false;
        agent.policy = Some(policy);
    }
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
