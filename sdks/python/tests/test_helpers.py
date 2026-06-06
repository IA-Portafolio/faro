"""Unit tests for the pure helper functions in faro_sdk.

These were previously only exercised indirectly through the HTTP-server
fixture tests. Pinning them directly gives fast, focused regression coverage
for the trace-context parsing and scrubbing logic that the wire format and
security guarantees depend on.
"""

import re

import faro_sdk
from faro_sdk import (
    _normalize_hex,
    _normalize_trace_context,
    _parse_traceparent,
    _scrub_entry,
    _scrub_string,
    _stringify_attr,
)

REDACTED = "[REDACTED]"


class TestStringifyAttr:
    def test_passthrough_string(self):
        assert _stringify_attr("hello") == "hello"

    def test_none_becomes_empty_string(self):
        assert _stringify_attr(None) == ""

    def test_scalars(self):
        assert _stringify_attr(42) == "42"
        assert _stringify_attr(3.5) == "3.5"
        assert _stringify_attr(True) == "True"

    def test_dict_serialized_as_json(self):
        assert _stringify_attr({"a": 1}) == '{"a": 1}'

    def test_list_serialized_as_json(self):
        assert _stringify_attr([1, 2]) == "[1, 2]"


class TestParseTraceparent:
    def test_valid_traceparent(self):
        tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        assert _parse_traceparent(tp) == {
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "span_id": "00f067aa0ba902b7",
        }

    def test_lowercases_hex(self):
        tp = "00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-01"
        out = _parse_traceparent(tp)
        assert out["trace_id"] == "4bf92f3577b34da6a3ce929d0e0e4736"
        assert out["span_id"] == "00f067aa0ba902b7"

    def test_strips_surrounding_whitespace(self):
        tp = "  00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01  "
        assert _parse_traceparent(tp) is not None

    def test_all_zero_trace_id_rejected(self):
        tp = "00-00000000000000000000000000000000-00f067aa0ba902b7-01"
        assert _parse_traceparent(tp) is None

    def test_all_zero_span_id_rejected(self):
        tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"
        assert _parse_traceparent(tp) is None

    def test_malformed_returns_none(self):
        assert _parse_traceparent("not-a-traceparent") is None
        assert _parse_traceparent("") is None
        # wrong trace-id length
        assert _parse_traceparent("00-abc-00f067aa0ba902b7-01") is None


class TestNormalizeHex:
    def test_valid_32_char_trace_id(self):
        v = "4bf92f3577b34da6a3ce929d0e0e4736"
        assert _normalize_hex(v, 32) == v

    def test_lowercases_and_strips(self):
        assert _normalize_hex("  00F067AA0BA902B7  ", 16) == "00f067aa0ba902b7"

    def test_wrong_length_rejected(self):
        assert _normalize_hex("abcd", 32) is None

    def test_non_hex_rejected(self):
        assert _normalize_hex("z" * 32, 32) is None

    def test_all_zero_rejected(self):
        assert _normalize_hex("0" * 16, 16) is None

    def test_non_string_rejected(self):
        assert _normalize_hex(12345, 16) is None
        assert _normalize_hex(None, 16) is None


class TestNormalizeTraceContext:
    def test_none_input(self):
        assert _normalize_trace_context(None) is None

    def test_string_traceparent(self):
        tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        assert _normalize_trace_context(tp) == {
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "span_id": "00f067aa0ba902b7",
        }

    def test_dict_with_traceparent_key(self):
        tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        assert _normalize_trace_context({"traceparent": tp}) == {
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "span_id": "00f067aa0ba902b7",
        }

    def test_dict_with_explicit_ids(self):
        out = _normalize_trace_context(
            {
                "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
                "span_id": "00f067aa0ba902b7",
            }
        )
        assert out == {
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "span_id": "00f067aa0ba902b7",
        }

    def test_dict_with_only_trace_id(self):
        out = _normalize_trace_context({"trace_id": "4bf92f3577b34da6a3ce929d0e0e4736"})
        assert out == {"trace_id": "4bf92f3577b34da6a3ce929d0e0e4736"}

    def test_dict_missing_trace_id_rejected(self):
        assert _normalize_trace_context({"span_id": "00f067aa0ba902b7"}) is None

    def test_non_dict_non_str_rejected(self):
        assert _normalize_trace_context(12345) is None


class TestScrubString:
    def test_redacts_email(self):
        rx = [faro_sdk._SCRUB_REGEXES["email"]]
        assert _scrub_string("contact me at a@b.com please", rx) == (
            f"contact me at {REDACTED} please"
        )

    def test_redacts_jwt(self):
        rx = [faro_sdk._SCRUB_REGEXES["jwt"]]
        jwt = "eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM"
        assert _scrub_string(f"token={jwt}", rx) == f"token={REDACTED}"

    def test_redacts_api_key(self):
        rx = [faro_sdk._SCRUB_REGEXES["api-key"]]
        assert _scrub_string("key sk-abcdef0123456789 used", rx) == f"key {REDACTED} used"

    def test_no_regex_leaves_string_untouched(self):
        assert _scrub_string("nothing here", []) == "nothing here"


class TestScrubEntry:
    def test_redacts_sensitive_keys(self):
        entry = {"attributes": {"password": "hunter2", "user": "alice"}}
        _scrub_entry(entry, ["password"], [])
        assert entry["attributes"]["password"] == REDACTED
        assert entry["attributes"]["user"] == "alice"

    def test_key_match_is_case_insensitive_and_substring(self):
        entry = {"attributes": {"Authorization": "Bearer x", "X-API_KEY": "abc"}}
        _scrub_entry(entry, ["authorization", "api_key"], [])
        assert entry["attributes"]["Authorization"] == REDACTED
        assert entry["attributes"]["X-API_KEY"] == REDACTED

    def test_regex_scrubs_string_values(self):
        rx = [faro_sdk._SCRUB_REGEXES["email"]]
        entry = {"attributes": {"note": "ping a@b.com"}}
        _scrub_entry(entry, [], rx)
        assert entry["attributes"]["note"] == f"ping {REDACTED}"

    def test_regex_scrubs_message(self):
        rx = [faro_sdk._SCRUB_REGEXES["email"]]
        entry = {"message": "user a@b.com signed up", "attributes": {}}
        _scrub_entry(entry, [], rx)
        assert entry["message"] == f"user {REDACTED} signed up"

    def test_handles_missing_attributes(self):
        entry = {"message": "hi"}
        # must not raise when there is no attributes dict
        _scrub_entry(entry, ["password"], [])
        assert entry["message"] == "hi"


class TestScrubRegexPresets:
    def test_all_presets_are_compiled_patterns(self):
        for name, rx in faro_sdk._SCRUB_REGEXES.items():
            assert isinstance(rx, re.Pattern), name

    def test_credit_card_is_opt_in_preset(self):
        assert "credit-card" in faro_sdk._SCRUB_REGEXES


class TestSpanName:
    def test_method_and_path(self):
        from faro_sdk.middleware import _span_name

        assert _span_name("GET", "/users") == "GET /users"

    def test_strips_when_path_empty(self):
        from faro_sdk.middleware import _span_name

        assert _span_name("GET", "").strip() == "GET"
