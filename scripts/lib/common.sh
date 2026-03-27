#!/usr/bin/env bash
# Shared utility functions for markkit scripts

set -euo pipefail

# Check if a git identity value looks like an AI agent
git_identity_looks_agent_like() {
    local value="$1"
    local lower
    lower=$(echo "$value" | tr '[:upper:]' '[:lower:]')
    case "$lower" in
        *claude*|*copilot*|*gpt*|*bot*|*agent*|*ai*|*assistant*)
            return 0
            ;;
    esac
    return 1
}

# Resolve a human git identity for commits
resolve_commit_git_identity() {
    local name email source

    # Try git config first
    name=$(git config user.name 2>/dev/null || true)
    email=$(git config user.email 2>/dev/null || true)

    if [[ -n "$name" && -n "$email" ]]; then
        if ! git_identity_looks_agent_like "$name" && ! git_identity_looks_agent_like "$email"; then
            RESOLVED_NAME="$name"
            RESOLVED_EMAIL="$email"
            RESOLVED_SOURCE="git-config"
            return 0
        fi
    fi

    # Fall back to environment variables
    name="${GIT_USER_NAME:-}"
    email="${GIT_USER_EMAIL:-}"

    if [[ -n "$name" && -n "$email" ]]; then
        RESOLVED_NAME="$name"
        RESOLVED_EMAIL="$email"
        RESOLVED_SOURCE="env"
        return 0
    fi

    echo "ERROR: No human git identity found." >&2
    echo "Set GIT_USER_NAME and GIT_USER_EMAIL to a real user before committing." >&2
    return 1
}

# Configure git identity if current one looks agent-like
configure_commit_git_identity_if_needed() {
    if resolve_commit_git_identity; then
        if [[ "$RESOLVED_SOURCE" == "env" ]]; then
            git config user.name "$RESOLVED_NAME"
            git config user.email "$RESOLVED_EMAIL"
        fi
    fi
}
