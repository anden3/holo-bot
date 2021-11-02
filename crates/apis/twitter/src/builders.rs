use crate::{errors::Error, ProductTrack, Rule};

pub struct RuleBuilder {
    rules: Vec<Rule>,
    track: ProductTrack,
}

impl RuleBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "academic_research_track")]
    pub fn with_track(track: ProductTrack) -> Self {
        Self {
            track,
            ..Self::default()
        }
    }

    pub fn add_rule<R: Into<Rule>>(&mut self, rule: R) -> &mut Self {
        self.rules.push(rule.into());
        self
    }

    pub fn add_rules<R, It>(&mut self, rules: It) -> &mut Self
    where
        R: Into<Rule>,
        It: IntoIterator<Item = R>,
    {
        self.rules.extend(rules.into_iter().map(|r| r.into()));
        self
    }

    pub fn build(self) -> Result<Vec<Rule>, Error> {
        let (max_rule_count, max_rule_length) = match &self.track {
            ProductTrack::Standard => (25, 512),
            ProductTrack::AcademicResearch => (1000, 1024),
        };

        if self.rules.len() > max_rule_count {
            return Err(Error::RuleLimitExceeded {
                count: self.rules.len(),
                limit: max_rule_count,
            });
        }

        if let Some(r) = self.rules.iter().find(|r| r.value.len() > max_rule_length) {
            return Err(Error::RuleLengthExceeded {
                rule: r.value.clone(),
                length: r.value.len(),
                limit: max_rule_length,
            });
        }

        Ok(self.rules)
    }
}

impl Default for RuleBuilder {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            track: ProductTrack::default(),
        }
    }
}

impl IntoIterator for RuleBuilder {
    type Item = Rule;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.rules.into_iter()
    }
}
