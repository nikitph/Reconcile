#!/usr/bin/env python3
"""Executable script for the packaged clinical trials example."""

from reconcile.examples.clinical_trials_system import create_clinical_trials_system


if __name__ == "__main__":
    print(create_clinical_trials_system().export_spec())
