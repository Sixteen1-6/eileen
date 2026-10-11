"""Routing policy: defaults per task kind, and the spoken override that beats them."""

from enum import StrEnum
from pathlib import Path
from typing import Literal, Self

import yaml
from pydantic import BaseModel, Field

CostTier = Literal["cheap", "standard", "frontier"]


class PermissionTier(StrEnum):
    """How an action is gated before it runs."""

    immediate = "immediate"
    confirm = "confirm"
    blocked = "blocked"


class RouteRule(BaseModel):
    """Where a task kind goes by default."""

    kind: str
    harness: str = Field(description="claude, codex, gemini, opencode or local.")
    model: str
    cost_tier: CostTier = "standard"
    local_only: bool = False


class RoutingPolicy(BaseModel):
    """A list of rules plus the rule used when no kind matches."""

    rules: list[RouteRule] = Field(default_factory=list)
    default: RouteRule

    @classmethod
    def from_yaml(cls, path: str | Path) -> Self:
        """Load a policy from a YAML file shaped like src/eileen/policy/default.yaml."""
        raw = yaml.safe_load(Path(path).read_text(encoding="utf-8")) or {}
        default = dict(raw.get("default") or {})
        default.setdefault("kind", "default")
        rules = [dict(rule) for rule in raw.get("rules") or []]
        return cls.model_validate({"rules": rules, "default": default})

    def rule_for(self, kind: str) -> RouteRule:
        """The first rule whose kind matches, or the default."""
        for rule in self.rules:
            if rule.kind == kind:
                return rule
        return self.default


class Override(BaseModel):
    """A one-off instruction from the user that wins over policy for this request."""

    harness: str | None = None
    model: str | None = None
    local_only: bool | None = None
    reason: str = Field(default="", description="What the user said, for the audit log.")


def default_policy_path() -> Path:
    """Path of the policy file shipped inside the package."""
    return Path(__file__).resolve().parent.parent / "policy" / "default.yaml"


def resolve(policy: RoutingPolicy, kind: str, override: Override | None = None) -> RouteRule:
    """Pick the rule for a kind, then apply any override fields the user set."""
    rule = policy.rule_for(kind)
    if override is None:
        return rule
    changes = {
        name: value
        for name, value in override.model_dump(exclude={"reason"}).items()
        if value is not None
    }
    if not changes:
        return rule
    return rule.model_copy(update=changes)
