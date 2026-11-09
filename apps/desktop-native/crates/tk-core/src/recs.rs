//! Local recommendation engine (wave B, worker **b**; docs/redesign/16-parity-wave-b-contract.md section 3.5;
//! checklist `home.recs.for-you`, `home.recs.explanations`, `home.recs.feedback`, `home.recs.atlas`; classic
//! `hooks/useRecommendationEngine.ts`, `atlas/*`). UI-free, pure functions over the local history + list entries:
//! `TasteProfile::from(&[Seen]) -> TasteProfile` (genre / studio / era / tag weights normalised to 0..=1),
//! `score(&TasteProfile, &Candidate) -> (f32, Vec<Reason>)` (cosine over the weight vectors + a score prior; reasons
//! are the top contributing features, rendered by the UI with i18n), `atlas(&[Candidate]) -> Vec<(f32, f32, cluster)>`
//! (2-D projection: PCA over the genre vectors is enough, no ML crate; clusters by dominant genre). Feedback rows
//! (`store/home.rs::recs_feedback`) remove `NotInterested` titles and boost `Like`d features.

/// Weighted feature vector of the viewer's taste (b fills it).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TasteProfile {
    pub genres: Vec<(String, f32)>,
    pub studios: Vec<(String, f32)>,
    pub eras: Vec<(String, f32)>,
    pub tags: Vec<(String, f32)>,
    pub sample_size: u32,
}

/// Decade label of a year (`1999 -> "1990s"`); pure, used for the `eras` feature.
pub fn era_of(year: u16) -> String {
    format!("{}s", year / 10 * 10)
}

#[cfg(test)]
mod tests {
    #[test]
    fn eras_are_decades() {
        assert_eq!(super::era_of(1999), "1990s");
        assert_eq!(super::era_of(2024), "2020s");
    }
}
