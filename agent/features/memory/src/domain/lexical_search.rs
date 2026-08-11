use std::collections::{HashMap, HashSet};

use super::{search_tie_break_score, MemoryEntry};
use crate::{MemoryLocation, MemorySearchHit, MemorySearchQuery};

const EXACT_MATCH_BOOST: f64 = 100.0;
const CONTENT_WEIGHT: f64 = 3.0;
const TAG_WEIGHT: f64 = 2.0;
const FACET_WEIGHT: f64 = 1.0;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

pub(crate) fn rank_explicit_search<'a>(
    candidates: impl IntoIterator<Item = (&'a MemoryEntry, MemoryLocation)>,
    query: &MemorySearchQuery,
) -> Vec<MemorySearchHit> {
    let query_terms = tokenize(&query.text);
    if query_terms.is_empty() || query.limit == 0 {
        return Vec::new();
    }

    let documents = candidates
        .into_iter()
        .map(|(entry, location)| LexicalDocument::new(entry, location))
        .collect::<Vec<_>>();
    if documents.is_empty() {
        return Vec::new();
    }

    let average_length = documents
        .iter()
        .map(|document| document.length as f64)
        .sum::<f64>()
        / documents.len() as f64;
    let document_frequencies = document_frequencies(&documents, &query_terms);
    let total_documents = documents.len();
    let normalized_query = query.text.trim().to_lowercase();

    let mut hits = documents
        .into_iter()
        .filter_map(|document| {
            let score = score_document(
                &document,
                &query_terms,
                &document_frequencies,
                total_documents,
                average_length,
                &normalized_query,
            );
            (score > 0.0).then(|| MemorySearchHit {
                entry: document.entry.clone(),
                location: document.location,
                outdated: document.entry.outdated,
                ttl_expired: document.entry.is_ttl_expired(query.now),
                relevance: Some(score),
            })
        })
        .collect::<Vec<_>>();

    hits.sort_by(|left, right| {
        right
            .relevance
            .partial_cmp(&left.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                search_tie_break_score(&right.entry, query.now)
                    .cmp(&search_tie_break_score(&left.entry, query.now))
            })
            .then_with(|| left.entry.id.cmp(&right.entry.id))
    });
    hits.truncate(query.limit);
    hits
}

struct LexicalDocument<'a> {
    entry: &'a MemoryEntry,
    location: MemoryLocation,
    weighted_term_frequencies: HashMap<String, f64>,
    unique_terms: HashSet<String>,
    length: usize,
}

impl<'a> LexicalDocument<'a> {
    fn new(entry: &'a MemoryEntry, location: MemoryLocation) -> Self {
        let content_terms = tokenize(&entry.content);
        let tag_terms = entry
            .tags
            .iter()
            .flat_map(|tag| tokenize(tag))
            .collect::<Vec<_>>();
        let facet_terms = tokenize(&format!("{:?} {:?}", entry.category, entry.layer));
        let mut weighted_term_frequencies = HashMap::new();
        add_weighted_terms(
            &mut weighted_term_frequencies,
            &content_terms,
            CONTENT_WEIGHT,
        );
        add_weighted_terms(&mut weighted_term_frequencies, &tag_terms, TAG_WEIGHT);
        add_weighted_terms(&mut weighted_term_frequencies, &facet_terms, FACET_WEIGHT);
        let unique_terms = weighted_term_frequencies.keys().cloned().collect();
        let length = content_terms.len() + tag_terms.len() + facet_terms.len();
        Self {
            entry,
            location,
            weighted_term_frequencies,
            unique_terms,
            length,
        }
    }
}

fn add_weighted_terms(frequencies: &mut HashMap<String, f64>, terms: &[String], weight: f64) {
    for term in terms {
        *frequencies.entry(term.clone()).or_default() += weight;
    }
}

fn document_frequencies(
    documents: &[LexicalDocument<'_>],
    query_terms: &[String],
) -> HashMap<String, (usize, usize)> {
    query_terms
        .iter()
        .map(|term| {
            let matching_documents = documents
                .iter()
                .filter(|document| document.unique_terms.contains(term))
                .count();
            (term.clone(), (matching_documents, documents.len()))
        })
        .collect()
}

fn score_document(
    document: &LexicalDocument<'_>,
    query_terms: &[String],
    document_frequencies: &HashMap<String, (usize, usize)>,
    total_documents: usize,
    average_length: f64,
    normalized_query: &str,
) -> f64 {
    let exact_boost = if document.entry.content.trim().to_lowercase() == normalized_query {
        EXACT_MATCH_BOOST
    } else {
        0.0
    };
    let lexical_score = query_terms
        .iter()
        .map(|term| {
            let term_frequency = document
                .weighted_term_frequencies
                .get(term)
                .copied()
                .unwrap_or_default();
            if term_frequency == 0.0 {
                return 0.0;
            }
            let document_frequency = document_frequencies
                .get(term)
                .map_or(0, |(frequency, _)| *frequency);
            let inverse_document_frequency = ((total_documents as f64 - document_frequency as f64
                + 0.5)
                / (document_frequency as f64 + 0.5)
                + 1.0)
                .ln();
            let length_normalization = if average_length > 0.0 {
                1.0 - BM25_B + BM25_B * document.length as f64 / average_length
            } else {
                1.0
            };
            inverse_document_frequency * term_frequency * (BM25_K1 + 1.0)
                / (term_frequency + BM25_K1 * length_normalization)
        })
        .sum::<f64>();
    exact_boost + lexical_score
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}
