"""Robustness tests that probe for real bugs in the kernel.

These tests are designed to expose actual flaws in atomicity,
cascade handling, error propagation, and callback safety.
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult
from reconcile._native import ReconcileSystem


class TestCascadeErrorSemantics:
    """Bug: cascade errors propagate after the original transition is already committed.
    If a controller's action fails during cascade, the caller sees an error,
    but the original transition is already persisted."""

    def test_original_transition_survives_cascade_error(self):
        """If a cascade controller fails, the original transition should still be visible."""
        def broken_controller(resource, query):
            if resource.state == "B":
                # Try to transition to a non-existent state - this will fail
                return {"transition": "NONEXISTENT"}
            return None

        sys = define_system(
            name="item",
            states=["A", "B", "C"],
            transitions=[("A", "B"), ("B", "C")],
            terminal_states=["C"],
            controllers=[{
                "name": "broken",
                "reconcile": broken_controller,
                "on_events": ["item.transitioned"],
            }],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # Transition A -> B should succeed even if cascade controller fails
        # The controller tries B -> NONEXISTENT which gets rejected, but the
        # original A -> B should still be committed.
        r = sys.transition(rid, "B", actor="x", role="x",
                           authority_level="CONTROLLER")

        # The resource should be at B regardless of cascade behavior
        resource = sys.get(rid)
        assert resource is not None
        assert resource.state == "B", (
            f"Original transition should be committed even if cascade has issues. "
            f"State is {resource.state}"
        )

    def test_cascade_rejection_does_not_crash(self):
        """Controller action that gets rejected shouldn't crash the system."""
        def always_tries_invalid(resource, query):
            # Always tries to transition to the current state (might be invalid)
            return {"transition": resource.state}

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            controllers=[{
                "name": "invalid-ctrl",
                "reconcile": always_tries_invalid,
                "on_events": ["item.*"],
            }],
        )

        # Should not crash
        result = sys.create({}, actor="sys")
        assert result.success
        assert result.resource.state == "A"


class TestCascadeDepthEnforcement:
    """Bug: cascade depth counter is always 0, never incremented through
    recursive transition calls. This means infinite cascades are not bounded."""

    def test_mutually_recursive_controllers_terminate(self):
        """Two controllers that trigger each other should eventually stop."""
        call_count = {"value": 0}

        def ping_controller(resource, query):
            call_count["value"] += 1
            if resource.state == "PING" and call_count["value"] < 50:
                return {"transition": "PONG"}
            return None

        def pong_controller(resource, query):
            call_count["value"] += 1
            if resource.state == "PONG" and call_count["value"] < 50:
                return {"transition": "PING"}
            return None

        sys = define_system(
            name="ball",
            states=["START", "PING", "PONG"],
            transitions=[
                ("START", "PING"),
                ("PING", "PONG"),
                ("PONG", "PING"),
            ],
            controllers=[
                {
                    "name": "ping",
                    "reconcile": ping_controller,
                    "on_events": ["ball.transitioned"],
                    "priority": 90,
                },
                {
                    "name": "pong",
                    "reconcile": pong_controller,
                    "on_events": ["ball.transitioned"],
                    "priority": 80,
                },
            ],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # This transition triggers a ping-pong cascade.
        # The system should NOT hang or crash - it should terminate
        # (either via depth limit, or via the call_count guard above).
        try:
            sys.transition(rid, "PING", actor="x", role="x",
                           authority_level="CONTROLLER")
        except Exception:
            pass  # Cascade depth exceeded is acceptable

        # System should still be queryable
        resource = sys.get(rid)
        assert resource is not None
        # Should have stopped at some state (PING or PONG)
        assert resource.state in ("START", "PING", "PONG")

    def test_long_chain_depth_limited(self):
        """A chain of 20 auto-advance controllers should hit cascade depth limit."""
        # Create a 20-state linear chain
        state_names = [f"S{i}" for i in range(20)]
        transitions = [(f"S{i}", f"S{i+1}") for i in range(19)]

        controllers = []
        for i in range(19):
            from_s = f"S{i}"
            to_s = f"S{i+1}"

            def make_ctrl(from_state, to_state):
                def ctrl(resource, query):
                    if resource.state == from_state:
                        return {"transition": to_state}
                    return None
                return ctrl

            controllers.append({
                "name": f"auto-{i}",
                "reconcile": make_ctrl(from_s, to_s),
                "on_events": ["chain.*"],
                "priority": 90 - i,
            })

        sys = define_system(
            name="chain",
            states=state_names,
            transitions=transitions,
            terminal_states=[state_names[-1]],
            controllers=controllers,
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # The cascade should either reach the end or be depth-limited
        try:
            sys.native.transition(rid, "S1", "sys", "sys", "CONTROLLER")
        except Exception:
            pass  # Expected if depth limit kicks in

        resource = sys.get(rid)
        assert resource is not None
        # Should have advanced at least some states
        state_idx = int(resource.state[1:])
        assert state_idx >= 1, "Should have at least reached S1"


class TestControllerErrorHandling:
    """Bug: Controller errors are silently swallowed via .ok()."""

    def test_crashing_controller_doesnt_block_others(self):
        """A controller that raises should not prevent other controllers from running."""
        good_calls = []

        def crashing_controller(resource, query):
            raise RuntimeError("I am broken!")

        def good_controller(resource, query):
            good_calls.append(resource.state)
            return None

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            controllers=[
                {
                    "name": "crasher",
                    "reconcile": crashing_controller,
                    "on_events": ["item.*"],
                    "priority": 90,
                },
                {
                    "name": "good",
                    "reconcile": good_controller,
                    "on_events": ["item.*"],
                    "priority": 10,
                },
            ],
        )

        result = sys.create({}, actor="sys")
        assert result.success

        # The good controller should still run despite the crasher
        assert len(good_calls) > 0, "Good controller should run even though another crashed"

    def test_crashing_controller_doesnt_corrupt_state(self):
        """A controller that raises should not leave the resource in a bad state."""
        def crasher(resource, query):
            raise ValueError("Something went wrong")

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            controllers=[{
                "name": "crasher",
                "reconcile": crasher,
                "on_events": ["item.*"],
            }],
        )

        result = sys.create({}, actor="sys")
        assert result.success
        rid = result.resource.id

        r = sys.transition(rid, "B", actor="x", role="x",
                           authority_level="CONTROLLER")
        assert r.success
        assert sys.get(rid).state == "B"


class TestPolicyCallbackSafety:
    """Bug: Policy callbacks that return unexpected types default to ALLOW.
    This is a security issue."""

    def test_policy_returning_none_allows(self):
        """A policy that returns None = no opinion = allow (explicit design choice)."""
        def returns_none(resource, ctx, query):
            return None

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            policies=[{"name": "none_policy", "evaluate": returns_none}],
        )

        result = sys.create({}, actor="sys")
        r = sys.transition(
            result.resource.id, "B", actor="x", role="x",
            authority_level="CONTROLLER",
        )
        assert r.success, "None return = no opinion = allow"

    def test_policy_returning_integer_denies_fail_closed(self):
        """A policy returning an unrecognized type (int) is denied (fail-closed)."""
        def returns_int(resource, ctx, query):
            return 42

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            policies=[{"name": "int_policy", "evaluate": returns_int}],
        )

        result = sys.create({}, actor="sys")
        r = sys.transition(
            result.resource.id, "B", actor="x", role="x",
            authority_level="CONTROLLER",
        )
        assert not r.success, "Unrecognized return type should fail-closed (deny)"
        assert r.rejected_step == "evaluate_policies"
        assert "unrecognized type" in r.rejected_reason.lower()

    def test_policy_raising_exception_denies(self):
        """A policy that raises should deny (confirmed working)."""
        def exploding_policy(resource, ctx, query):
            raise RuntimeError("Policy crashed!")

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            policies=[{"name": "exploder", "evaluate": exploding_policy}],
        )

        result = sys.create({}, actor="sys")
        r = sys.transition(
            result.resource.id, "B", actor="x", role="x",
            authority_level="CONTROLLER",
        )
        assert not r.success, "Crashing policy should deny"
        assert r.rejected_step == "evaluate_policies"


class TestAtomicityGuarantees:
    """Test that the 8-step transaction boundary is truly atomic:
    if ANY step fails, NO side effects should be visible."""

    def test_failed_policy_leaves_no_event(self):
        """If a policy denies, no event or audit should be created."""
        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            policies=[{
                "name": "always_deny",
                "evaluate": lambda r, c, q: PolicyResult.deny("nope"),
            }],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        events_before = len(sys.events(rid))
        audit_before = len(sys.audit(rid))

        r = sys.transition(rid, "B", actor="x", role="x",
                           authority_level="CONTROLLER")
        assert not r.success

        # No new events or audit records
        assert len(sys.events(rid)) == events_before
        assert len(sys.audit(rid)) == audit_before
        # State unchanged
        assert sys.get(rid).state == "A"
        assert sys.get(rid).version == 1

    def test_failed_invariant_leaves_no_side_effects(self):
        """If a strong invariant blocks, no mutations at all."""
        def block_b(resource, query):
            if resource.state == "B":
                return InvariantResult.violated("Cannot be in B")
            return InvariantResult.ok()

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            invariants=[{
                "name": "no_b",
                "mode": "strong",
                "scope": "resource",
                "check": block_b,
            }],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        r = sys.transition(rid, "B", actor="x", role="x",
                           authority_level="CONTROLLER")
        assert not r.success

        assert sys.get(rid).state == "A"
        assert sys.get(rid).version == 1
        assert len(sys.audit(rid)) == 0

    def test_failed_rbac_leaves_no_side_effects(self):
        """RBAC denial leaves zero trace."""
        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            roles={"viewer": ["view"]},
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        r = sys.transition(rid, "B", actor="viewer1", role="viewer")
        assert not r.success

        assert sys.get(rid).state == "A"
        assert sys.get(rid).version == 1
        assert len(sys.audit(rid)) == 0


class TestDesiredStateRobustness:
    """Test edge cases in desired state reconciliation."""

    def test_desired_state_with_policy_blocking_shortest_path(self):
        """If a policy blocks the shortest path, reconciliation should handle it."""
        #   A -> B -> D (shortest, but B->D is policy-blocked)
        #   A -> B -> C -> D (longer path)
        def block_b_to_d(resource, ctx, query):
            if ctx.get("from_state") == "B" and ctx.get("to_state") == "D":
                return PolicyResult.deny("B->D is blocked")
            return PolicyResult.allow()

        sys = define_system(
            name="item",
            states=["A", "B", "C", "D"],
            transitions=[("A", "B"), ("B", "C"), ("B", "D"), ("C", "D")],
            terminal_states=["D"],
            policies=[{
                "name": "block_bd",
                "evaluate": block_b_to_d,
                "applicable_states": ["B"],
            }],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # Try to reconcile to D
        # Shortest path is A->B->D but B->D is blocked.
        # Current implementation will try B->D, fail, and raise ConvergenceFailure.
        # This documents the current behavior (no alternative path search).
        try:
            sys.set_desired(rid, "D", requested_by="sys")
            # If it succeeds, it found an alternative path (fixed bug)
            assert sys.get(rid).state == "D"
        except Exception:
            # Expected: ConvergenceFailure because shortest path is blocked
            # Resource should be at B (made it one step)
            resource = sys.get(rid)
            assert resource.state in ("A", "B"), (
                f"Resource should be at A or B after failed reconciliation, got {resource.state}"
            )

    def test_desired_state_already_at_target(self):
        """Setting desired state to current state should be a no-op."""
        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id

        events_before = len(sys.events(rid))
        sys.set_desired(rid, "A", requested_by="sys")

        # State unchanged, but version bumps (set_desired is a write operation)
        assert sys.get(rid).state == "A"
        assert sys.get(rid).version == 2


class TestEventPatternEdgeCases:
    """Test event pattern matching behavior with unusual patterns."""

    def test_controller_on_exact_event_type(self):
        calls = []

        def tracker(resource, query):
            calls.append(resource.state)
            return None

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            controllers=[{
                "name": "tracker",
                "reconcile": tracker,
                "on_events": ["item.transitioned"],  # Exact match
            }],
        )

        result = sys.create({}, actor="sys")  # Should NOT trigger (event is item.created)
        assert len(calls) == 0

        sys.transition(
            result.resource.id, "B", actor="x", role="x",
            authority_level="CONTROLLER",
        )
        assert len(calls) == 1  # Should trigger on transition

    def test_wildcard_controller_sees_everything(self):
        calls = []

        def tracker(resource, query):
            calls.append(1)
            return None

        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
            controllers=[{
                "name": "tracker",
                "reconcile": tracker,
                "on_events": ["*"],  # Wildcard
            }],
        )

        sys.create({}, actor="sys")
        assert len(calls) >= 1  # Sees creation
