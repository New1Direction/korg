"""
Entry point for `python -m thump`.

This allows running the harness directly without installing it as a package:

    python -m thump script run dev
    python -m thump package add hono --dev
"""

from .cli import main

if __name__ == "__main__":
    import sys
    sys.exit(main())
