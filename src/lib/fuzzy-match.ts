/**
 * In-process fuzzy matching for the GUI.
 * Shared between CLI and TUI — no external dependencies.
 *
 * Scoring: consecutive matches get bonus, word-boundary matches get bonus,
 * prefix matches get bonus, shorter strings score higher for same matches.
 */

interface FuzzyResult<T> {
  item: T;
  score: number;
  indices: number[];
}

const SCORE_MATCH = 16;
const SCORE_GAP = -3;
const SCORE_CONSECUTIVE = 8;
const SCORE_WORD_BOUNDARY = 10;
const SCORE_PREFIX = 12;

function isWordBoundary(text: string, i: number): boolean {
  if (i === 0) return true;
  const prev = text[i - 1];
  const curr = text[i];
  if (!prev || !curr) return false;
  if (prev === "/" || prev === "-" || prev === "_" || prev === ".") return true;
  if (prev === prev.toLowerCase() && curr === curr.toUpperCase()) return true;
  return false;
}

function fuzzyScore(query: string, text: string): { score: number; indices: number[] } | null {
  const queryLower = query.toLowerCase();
  const textLower = text.toLowerCase();

  // Quick check: all query chars must exist in text
  let qi = 0;
  for (let ti = 0; ti < textLower.length && qi < queryLower.length; ti++) {
    if (textLower[ti] === queryLower[qi]) qi++;
  }
  if (qi < queryLower.length) return null;

  // Score the match using a greedy approach with word-boundary preference
  const indices: number[] = [];
  let score = 0;
  let lastIdx = -1;

  qi = 0;
  // First pass: prefer word boundaries
  for (let ti = 0; ti < textLower.length && qi < queryLower.length; ti++) {
    if (textLower[ti] === queryLower[qi] && isWordBoundary(text, ti)) {
      indices.push(ti);
      score += SCORE_MATCH + SCORE_WORD_BOUNDARY;
      if (ti === qi) score += SCORE_PREFIX;
      if (lastIdx >= 0 && ti === lastIdx + 1) score += SCORE_CONSECUTIVE;
      lastIdx = ti;
      qi++;
    }
  }

  // Second pass: fill remaining with any match
  if (qi < queryLower.length) {
    // Reset and do full greedy match
    indices.length = 0;
    score = 0;
    lastIdx = -1;
    qi = 0;

    for (let ti = 0; ti < textLower.length && qi < queryLower.length; ti++) {
      if (textLower[ti] === queryLower[qi]) {
        indices.push(ti);
        score += SCORE_MATCH;
        if (isWordBoundary(text, ti)) score += SCORE_WORD_BOUNDARY;
        if (ti === qi) score += SCORE_PREFIX;
        if (lastIdx >= 0 && ti === lastIdx + 1) score += SCORE_CONSECUTIVE;
        else if (lastIdx >= 0) score += SCORE_GAP * (ti - lastIdx - 1);
        lastIdx = ti;
        qi++;
      }
    }

    if (qi < queryLower.length) return null;
  }

  // Shorter strings rank higher for equal scores
  score -= Math.floor(text.length / 10);

  return { score, indices };
}

export function fuzzyFilter<T>(items: T[], query: string, keyFn: (item: T) => string): FuzzyResult<T>[] {
  if (!query) {
    return items.map((item) => ({ item, score: 0, indices: [] }));
  }

  const results: FuzzyResult<T>[] = [];
  for (const item of items) {
    const text = keyFn(item);
    const result = fuzzyScore(query, text);
    if (result) {
      results.push({ item, score: result.score, indices: result.indices });
    }
  }

  results.sort((a, b) => b.score - a.score);
  return results;
}
