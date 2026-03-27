#!/usr/bin/env python3
"""Start the Reconcile API server.

Usage:
  python serve.py              # Loan OS (default)
  python serve.py --permit     # Building Permit System (23 states, 50 policies)
"""

import sys
import uvicorn
from reconcile.api import create_app

if "--permit" in sys.argv:
    from reconcile.examples.building_permit_system import create_building_permit_system
    system = create_building_permit_system()
    print("Loading: Building Permit System (23 states, 50 policies)")
else:
    from reconcile.examples.loan_operating_system import create_loan_operating_system
    system = create_loan_operating_system()
    print("Loading: Loan Operating System")

app = create_app(system.native)

if __name__ == "__main__":
    print("API: http://localhost:8000")
    print("UI:  http://localhost:3000")
    uvicorn.run(app, host="0.0.0.0", port=8000)
