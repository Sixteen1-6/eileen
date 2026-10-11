"""Allow `python -m eileen` to behave like the `eileen` console script."""

from eileen.cli import main

if __name__ == "__main__":
    raise SystemExit(main())
