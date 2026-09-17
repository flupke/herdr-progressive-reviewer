"""Offline safety checks for annotations and decision rules (no Jev calls)."""

import unittest

from history import SourceLabels
from score import Rule, Scoring


class AnnotationTests(unittest.TestCase):
    def test_attribute_changes_are_not_formatting(self):
        old = SourceLabels.rust(b"struct S {\n x: String,\n}\n")
        new = SourceLabels.rust(b"struct S {\n #[serde(default)]\n x: String,\n}\n")
        self.assertNotEqual(old.tokens(2, 3), new.tokens(2, 4))
        self.assertEqual(new.categories[2], "code")

    def test_blank_lines_and_comment_markers_in_strings_are_runtime_data(self):
        source = SourceLabels.rust(b'const S: &str = r#"first\n\n// runtime text\nlast"#;\n')
        self.assertEqual(source.categories[2], "code")
        self.assertEqual(source.categories[3], "code")
        self.assertIsNone(source.tokens(2, 4))

    def test_test_scope_is_bounded_and_imports_are_excluded(self):
        source = SourceLabels.rust(b'#[cfg(test)]\nmod tests {\n fn helper() { panic!(); }\n}\nuse x::y;\nfn runtime() { panic!(); }\n')
        self.assertEqual(source.categories[3], "test")
        self.assertEqual(source.categories[5], "import")
        self.assertEqual(source.categories[6], "code")

    def test_compound_cfg_requires_test_not_merely_mentions_it(self):
        for cfg in (b"all(test, unix)", b"all(unix, test)"):
            source = SourceLabels.rust(b'#[cfg(' + cfg + b')]\n#[path="test.rs"]\nmod test_helpers;\n')
            self.assertEqual(source.categories[1], "test")
            self.assertEqual(source.categories[2], "test")
        for cfg in (b"any(test, unix)", b"all(unix, any(test, feature=\"x\"))",
                    b'all(feature="x,test,y", unix)', b'all(test="x", unix)'):
            source = SourceLabels.rust(b'#[cfg(' + cfg + b')]\nfn runtime() {}\n')
            self.assertEqual(source.categories[2], "code")


class DecisionTests(unittest.TestCase):
    def test_confidence_cannot_turn_significant_into_excluded(self):
        answer = {"checklist": {"choice": "significant", "confidence": 0.99,
                  "probabilities": {"significant": 0.98, "insignificant": 0.01, "uncertain": 0.01}}}
        for rule in Rule.grid():
            if rule.prompt == "checklist":
                self.assertLess(rule.value(answer), rule.threshold)

    def test_a_review_veto_overrides_a_high_category_probability(self):
        answers = {n: {"noul": 0.1} for n in ("comment", "import", "formatting", "out_of_scope_file")}
        answers["comment"]["noul"] = 0.99
        answers["requires_review"] = {"noul": 0.95}
        self.assertLess(Rule("categories", "max_with_veto", 0.9).value(answers), 0.9)

    def test_false_exclusions_cannot_trade_for_efficiency(self):
        safe = dict(false_cases=0, false_lines=0, failures=0, oversized=0,
                    useful_lines_per_call=1, useful_lines=10, calls=10,
                    estimated_tokens=1000,
                    threshold=.9, prompt="terse", profile="rows", method="probability")
        unsafe = dict(safe, false_cases=1, false_lines=1, useful_lines_per_call=1000)
        self.assertLess(Scoring.rank(safe), Scoring.rank(unsafe))


if __name__ == "__main__":
    unittest.main()
