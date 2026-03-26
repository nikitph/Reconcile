"""Controller base class for Reconcile."""


class Controller:
    """Base class for reactive controllers.

    Subclass this and implement the reconcile() method.
    """

    name: str = ""
    priority: int = 50
    enforces: list[str] = []
    on_events: list[str] = []
    authority_level: str = "CONTROLLER"

    def reconcile(self, resource, ctx=None):
        """Called when a matching event fires or during reconciliation.

        Return:
            - None or "noop": no action
            - A string: transition to that state
            - {"transition": "STATE"}: transition to state
            - {"set_desired": "STATE"}: set desired state
        """
        raise NotImplementedError
