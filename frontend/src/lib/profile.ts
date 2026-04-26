import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./api";
import toast from "react-hot-toast";

// ─── Types ────────────────────────────────────────────────────────────────────

export interface DistinctiveWord {
  word: string;
  score: number;
}

export interface Phrase {
  phrase: string;
  count: number;
}

export interface LexicalProfile {
  type_token_ratio: number;
  avg_word_length: number;
  vocabulary_level: "basic" | "intermediate" | "advanced";
  distinctive_words: DistinctiveWord[];
  characteristic_bigrams: Phrase[];
  characteristic_trigrams: Phrase[];
  function_word_profile: Record<string, number>;
}

export interface SentenceLengthDistribution {
  "short (<10)": number;
  "medium (10-20)": number;
  "long (>20)": number;
}

export interface SentenceTypeMix {
  declarative: number;
  interrogative: number;
  exclamatory: number;
  fragment: number;
}

export interface PunctuationRhythm {
  comma_per_sentence: number;
  em_dash_per_1000_words: number;
  ellipsis_per_1000_words: number;
  semicolon_per_1000_words: number;
}

export interface SyntacticProfile {
  avg_sentence_length: number;
  sentence_length_distribution: SentenceLengthDistribution;
  sentence_type_mix: SentenceTypeMix;
  punctuation_rhythm: PunctuationRhythm;
  paragraph_length_avg_sentences: number;
}

export interface TopicEntry {
  label: string;
  weight: number;
  keywords: string[];
}

export interface EntityEntry {
  entity: string;
  count: number;
  kind: string;
}

export interface SentimentBaseline {
  polarity: number;
  subjectivity: number;
}

export interface SemanticProfile {
  top_topics: TopicEntry[];
  recurring_entities: EntityEntry[];
  sentiment_baseline: SentimentBaseline;
}

export interface StylisticProfile {
  opening_gambits: string[];
  sign_offs: string[];
  recurring_metaphors: string[];
  register: string;
  first_person_rate: number;
  contractions_rate: number;
}

export interface Exemplar {
  chunk_id: string;
  score: number;
  reason: string;
}

export interface CorpusStats {
  document_count: number;
  chunk_count: number;
  word_count: number;
  date_range: [string, string] | null;
}

export interface StyleProfile {
  version: 1;
  status: "ok" | "insufficient_corpus" | "pending";
  corpus?: CorpusStats;
  lexical?: LexicalProfile;
  syntactic?: SyntacticProfile;
  semantic?: SemanticProfile;
  stylistic?: StylisticProfile;
  exemplars?: Exemplar[];
  message?: string;
}

// ─── Hooks ────────────────────────────────────────────────────────────────────

export function usePersonaProfile(personaId: string) {
  return useQuery<StyleProfile, ApiError>({
    queryKey: ["personas", personaId, "profile"],
    queryFn: () => api<StyleProfile>(`/api/personas/${personaId}/profile`),
    enabled: Boolean(personaId),
    refetchInterval: (query) =>
      query.state.data?.status === "pending" ? 5000 : false,
  });
}

export function useEraProfile(personaId: string, eraId: string | null) {
  return useQuery<StyleProfile, ApiError>({
    queryKey: ["personas", personaId, "eras", eraId, "profile"],
    queryFn: () =>
      api<StyleProfile>(`/api/personas/${personaId}/eras/${eraId}/profile`),
    enabled: Boolean(personaId) && Boolean(eraId),
    refetchInterval: (query) =>
      query.state.data?.status === "pending" ? 5000 : false,
  });
}

export function useRecomputeProfile(personaId: string) {
  const qc = useQueryClient();
  return useMutation<{ status: string }, ApiError, void>({
    mutationFn: () =>
      api(`/api/personas/${personaId}/profile/recompute`, {
        method: "POST",
        headers: { "Idempotency-Key": crypto.randomUUID() },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "profile"] });
      toast.success("Profile recomputation queued.");
    },
    onError: (e) => toast.error(e.message || "Failed to queue recomputation."),
  });
}
