"""Run the rebuilt Listen and compare its observations with the mechanics sheet."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


APP = Path(__file__).resolve().parents[1]


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def compare(sheet, observations):
    if sheet["version"] != 1 or not sheet["values"]:
        raise ValueError("Expected a nonempty version 1 mechanics sheet")
    rows = []
    names = set()
    for row in sheet["values"]:
        name, expected = row["name"], row["value"]
        if (not isinstance(name, str) or not name or name in names
                or type(expected) not in (int, float, bool, str)
                or not (row.get("source") or row.get("estimated") is True)):
            raise ValueError(f"Invalid or duplicate mechanics entry: {name!r}")
        names.add(name)
        result = {"name": name, "expected": expected, "status": "unmeasured"}
        if "unit" in row:
            result["unit"] = row["unit"]
        if name in observations:
            observation = observations[name]
            observed, method = observation["value"], observation["method"]
            if type(observed) not in (int, float, bool, str) or not method:
                raise ValueError(f"Invalid observation: {name}")
            compatible = type(expected) is type(observed) or (
                type(expected) in (int, float) and type(observed) in (int, float)
            )
            result.update(observed=observed, method=method,
                          status="match" if compatible and observed == expected else "different")
        rows.append(result)
    if unknown := observations.keys() - names:
        raise ValueError(f"Observations without checklist entries: {', '.join(sorted(unknown))}")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, default=APP / "target/release/listen-rs.exe")
    parser.add_argument("--report", type=Path, help="Write the row-by-row results to a new JSON file")
    args = parser.parse_args()
    binary = args.executable.resolve(strict=True)
    digest = sha256(binary)
    sheet_path = APP / "mechanics.json"
    sheet_digest = sha256(sheet_path)
    sheet = read_json(sheet_path)
    with tempfile.TemporaryDirectory(prefix="listen-mechanics-") as directory:
        output = Path(directory) / "observations.json"
        subprocess.run([str(binary), "--measure-mechanics", str(output)], check=True, timeout=15)
        observations = read_json(output)
    if digest != sha256(binary) or sheet_digest != sha256(sheet_path):
        raise ValueError("The executable or mechanics sheet changed during the comparison")
    if not observations:
        raise ValueError("Listen returned no measurements")

    rows = compare(sheet, observations)
    totals = {status: sum(row["status"] == status for row in rows)
              for status in ("match", "different", "unmeasured")}
    if args.report:
        with args.report.open("x", encoding="utf-8") as stream:
            json.dump({"executable": binary.name, "executable_sha256": digest,
                       "sheet_sha256": sheet_digest, "totals": totals, "values": rows},
                      stream, indent=2, ensure_ascii=False)
            stream.write("\n")
    print(f"{len(rows)} checklist entries: {totals['match']} matched, "
          f"{totals['different']} differed, {totals['unmeasured']} unmeasured.")
    for row in rows:
        if row["status"] == "different":
            print(f"{row['name']}: expected {row['expected']!r}, observed {row['observed']!r}")
    return int(totals["different"] != 0)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(f"Mechanics comparison could not run: {error}")
