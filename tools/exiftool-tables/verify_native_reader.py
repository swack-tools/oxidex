"""Audit native reader source identity and independent execution observations.

This verifier does not import the compiler's expected Perl bodies. Its facts
come from a fresh native process and the live parent-loaded function refs.
"""
import native_reader_facts as facts


def mismatch(compiled_sha, snapshot):
    if (problem := facts.observation_failure(snapshot)) is not None:
        return problem
    try:
        isolated = snapshot.get("isolated_functions")
        loaded = snapshot.get("loaded_functions")
        if not isinstance(isolated, dict) or not isinstance(loaded, dict):
            return "missing native loaded or isolated reader functions"
        for key, name in facts.FUNCTIONS:
            if facts.source_fact(isolated.get(key), name) != facts.source_fact(loaded.get(key), name):
                return "live reader differs from independently executed reader"
        if compiled_sha != facts.fingerprint(snapshot):
            return "compiled reader source or byte-order observations differ from native"
    except facts.ReaderRefused as error:
        return str(error)
    return None
