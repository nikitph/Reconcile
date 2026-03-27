#!/usr/bin/env python3
"""Start the Reconcile Loan OS API server.

Usage: python serve.py
Then open http://localhost:3000 for the React UI (after `cd ui && npm start`)
"""

import uvicorn
from reconcile.examples.loan_operating_system import create_loan_operating_system
from reconcile.api import create_app

system = create_loan_operating_system()
app = create_app(system.native)

if __name__ == "__main__":
    print("Starting Reconcile API on http://localhost:8000")
    print("React UI should connect to http://localhost:3000")
    uvicorn.run(app, host="0.0.0.0", port=8000)
