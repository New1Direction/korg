#!/usr/bin/env bash
# korg demo simulation script — called by VHS tape
# Each function prints pre-scripted output with realistic timing

BOLD="\033[1m"
DIM="\033[2m"
CYAN="\033[36m"
GREEN="\033[32m"
YELLOW="\033[33m"
RED="\033[31m"
BLUE="\033[34m"
RESET="\033[0m"
GRAY="\033[90m"

korg_version() {
    echo -e "${BOLD}korg${RESET} 0.1.0"
}

korg_run() {
    local GOAL="$1"
    echo -e "${GRAY}2026-05-23T17:01:00Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg${RESET}: session_id=${GREEN}019e5333-efc9${RESET} mode=${YELLOW}balanced${RESET}"
    sleep 0.3
    echo -e "${GRAY}2026-05-23T17:01:00Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::leader${RESET}: spawning_swarm workers=${GREEN}[captain, harper, benjamin, lucas]${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:01Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::registry${RESET}: transition capability=${YELLOW}cognition_mode${RESET} state=${GREEN}balanced${RESET}"
    sleep 0.3
    echo -e "${GRAY}2026-05-23T17:01:01Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}1${RESET} event=${YELLOW}TransitionStarted${RESET} actor=coordinator"
    sleep 0.35
    echo -e "${GRAY}2026-05-23T17:01:02Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}2${RESET} event=${YELLOW}LeaseAcquired${RESET} capability=${CYAN}src/auth.rs${RESET} actor=benjamin"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:03Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::leader${RESET}: captain planning goal=\"${BOLD}${GOAL}${RESET}\""
    sleep 0.5
    echo -e "${GRAY}2026-05-23T17:01:04Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}3${RESET} event=${YELLOW}EffectStarted${RESET} actor=benjamin target=${CYAN}src/auth.rs${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:05Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::arena${RESET}: evaluating trajectory_score=${YELLOW}0.61${RESET} epistemic_score=${YELLOW}0.58${RESET}"
    sleep 0.35
    echo -e "${GRAY}2026-05-23T17:01:06Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}4${RESET} event=${YELLOW}EffectCompleted${RESET} actor=benjamin mutations=${CYAN}[src/auth.rs]${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:07Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::evaluator${RESET}: verdict=${RED}REVISE${RESET} semantic_entropy=${RED}0.72${RESET} reason=high_churn_detected"
    sleep 0.5
    echo -e "${GRAY}2026-05-23T17:01:08Z${RESET}  ${BOLD}${YELLOW}WARN${RESET} ${CYAN}korg::leader${RESET}: revision_requested doom_loop_risk=${YELLOW}moderate${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:09Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}5${RESET} event=${YELLOW}EffectRetrying${RESET} actor=benjamin retry_count=${RED}1${RESET}"
    sleep 0.6
}

korg_rewind() {
    local SEQ="$1"
    echo ""
    echo -e "  ${DIM}Rewinding capability journal to seq=${BOLD}${SEQ}${RESET}${DIM}...${RESET}"
    sleep 0.4
    echo -e "  ${DIM}Restoring workspace snapshot via git read-tree ${BOLD}(O(1))${RESET}${DIM}...${RESET}"
    sleep 0.3
    echo -e "  ${DIM}Rebuilding 3 projection read-models...${RESET}"
    sleep 0.4
    echo -e "  ${DIM}Resetting HLC clock: physical=${BOLD}17:01:03${RESET}${DIM} logical=${BOLD}${SEQ}${RESET}"
    sleep 0.5
    echo ""
    echo -e "  ${GREEN}✓${RESET} Rewound to ${BOLD}seq=${SEQ}${RESET}  workspace restored  clock aligned  projections rebuilt"
}

korg_fork_run() {
    local GOAL="$1"
    echo -e "${GRAY}2026-05-23T17:01:12Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::leader${RESET}: fork_branch branch_id=${GREEN}b91a4c2e${RESET} from_seq=${GREEN}3${RESET}"
    sleep 0.35
    echo -e "${GRAY}2026-05-23T17:01:12Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}4${RESET} event=${YELLOW}EffectStarted${RESET} actor=benjamin target=${CYAN}src/auth.rs${RESET} branch=${GREEN}b91a4c2e${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:14Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::log${RESET}: append seq=${GREEN}5${RESET} event=${YELLOW}EffectCompleted${RESET} actor=benjamin mutations=${CYAN}[src/auth.rs, src/middleware.rs]${RESET}"
    sleep 0.4
    echo -e "${GRAY}2026-05-23T17:01:15Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::arena${RESET}: trajectory_score=${GREEN}0.91${RESET} epistemic_score=${GREEN}0.89${RESET} verdict=${GREEN}${BOLD}ACCEPT${RESET}"
    sleep 0.5
    echo -e "${GRAY}2026-05-23T17:01:16Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::leader${RESET}: campaign_complete outcome=${GREEN}success${RESET} tx_id=${DIM}019e5334-3ebd${RESET}"
    sleep 0.6
    echo -e "${GRAY}2026-05-23T17:01:16Z${RESET}  ${BOLD}${BLUE}INFO${RESET} ${CYAN}korg::runtime${RESET}: cleanup session_id=${DIM}019e5333-efc9${RESET} destroyed=0"
}

case "$1" in
    version)  korg_version ;;
    run)      korg_run "$2" ;;
    rewind)   korg_rewind "$2" ;;
    fork)     korg_fork_run "$2" ;;
    *) echo "usage: korg-demo.sh [version|run|rewind|fork] [arg]" ;;
esac
