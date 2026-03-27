#!/usr/bin/env python3
"""Executable script for the packaged procurement example."""

from reconcile.examples.procurement_system import create_procurement_system


if __name__ == "__main__":
    print(create_procurement_system().export_spec())
