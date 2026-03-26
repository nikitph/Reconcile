"""Test harness for Reconcile systems."""


class SystemTestHarness:
    """Test helper for asserting system behavior."""

    def __init__(self, system):
        self._system = system

    def assert_transition_succeeds(self, resource_id: str, to_state: str, **kwargs):
        """Assert that a transition succeeds."""
        result = self._system.transition(resource_id, to_state, **kwargs)
        assert result.success, (
            f"Expected transition to {to_state} to succeed, "
            f"but was rejected at step '{result.rejected_step}': {result.rejected_reason}"
        )
        return result

    def assert_transition_blocked(self, resource_id: str, to_state: str,
                                  step: str | None = None, **kwargs):
        """Assert that a transition is blocked."""
        result = self._system.transition(resource_id, to_state, **kwargs)
        assert not result.success, f"Expected transition to {to_state} to be blocked, but it succeeded"
        if step:
            assert result.rejected_step == step, (
                f"Expected rejection at step '{step}', got '{result.rejected_step}'"
            )
        return result

    def assert_invariant_holds(self, resource_id: str):
        """Assert that the resource is in a valid state (no strong invariant violations)."""
        resource = self._system.get(resource_id)
        assert resource is not None, f"Resource {resource_id} not found"
        return resource

    def assert_state(self, resource_id: str, expected_state: str):
        """Assert that a resource is in the expected state."""
        resource = self._system.get(resource_id)
        assert resource is not None, f"Resource {resource_id} not found"
        assert resource.state == expected_state, (
            f"Expected state '{expected_state}', got '{resource.state}'"
        )
        return resource
