"""Summarize LLVM line coverage for the shared and command modules."""

import json
import sys


def summarize(report):
    totals = {"common": [0, 0], "commands": [0, 0]}
    for data in report["data"]:
        for file in data["files"]:
            name = file["filename"].replace("\\", "/")
            for scope, total in totals.items():
                if f"/src/{scope}/" in name:
                    lines = file["summary"]["lines"]
                    total[0] += lines["covered"]
                    total[1] += lines["count"]
    return totals


if __name__ == "__main__":
    with open(sys.argv[1], encoding="utf-8") as stream:
        totals = summarize(json.load(stream))
    for scope, (covered, count) in totals.items():
        if not count:
            raise SystemExit(f"No coverage data for src/{scope}/")
        print(f"src/{scope}/: {covered}/{count} lines ({covered / count:.2%})")
