import { useState } from "react";
import { useParams, Link } from "react-router-dom";
import { usePersona, useErasList, type Era } from "@/lib/personas";
import {
  usePersonaProfile,
  useEraProfile,
  useRecomputeProfile,
  type StyleProfile,
  type LexicalProfile,
  type SyntacticProfile,
  type SemanticProfile,
  type StylisticProfile,
  type Exemplar,
} from "@/lib/profile";
import { RelationBadge } from "@/components/ui/badge";
import {
  RefreshCw,
  Copy,
  ChevronDown,
  ChevronUp,
  BookOpen,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function pct(n: number): string {
  return `${(n * 100).toFixed(1)}%`;
}

function fmt(n: number, decimals = 2): string {
  return n.toFixed(decimals);
}

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function Card({
  title,
  children,
  className,
}: {
  title: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-5",
        className,
      )}
    >
      <h2 className="text-sm font-semibold text-[var(--text)] mb-4">{title}</h2>
      {children}
    </div>
  );
}

function StatChip({ label, value }: { label: string; value: string }) {
  return (
    <div className="inline-flex items-center gap-1 bg-[var(--bg)] border border-[var(--border)] rounded-full px-3 py-1 text-xs">
      <span className="text-[var(--text-subtle)]">{label}</span>
      <span className="font-medium text-[var(--text)]">{value}</span>
    </div>
  );
}

function WordChip({ word, score }: { word: string; score: number }) {
  const size =
    score > 5
      ? "text-sm font-semibold"
      : score > 2
        ? "text-xs font-medium"
        : "text-xs";
  return (
    <span
      className={cn(
        "inline-block font-mono bg-[var(--bg)] border border-[var(--border)] rounded px-2 py-0.5 mr-1 mb-1 text-[var(--text)]",
        size,
      )}
      title={`score: ${score.toFixed(2)}`}
    >
      {word}
    </span>
  );
}

function StackedBar({
  segments,
}: {
  segments: { label: string; value: number; color: string }[];
}) {
  return (
    <div className="flex h-3 rounded overflow-hidden w-full gap-px">
      {segments.map((s) => (
        <div
          key={s.label}
          className={cn("transition-all", s.color)}
          style={{ width: `${(s.value * 100).toFixed(1)}%` }}
          title={`${s.label}: ${pct(s.value)}`}
        />
      ))}
    </div>
  );
}

function WeightBar({
  label,
  value,
  keywords,
}: {
  label: string;
  value: number;
  keywords: string[];
}) {
  return (
    <div className="mb-3">
      <div className="flex items-center justify-between mb-1">
        <span className="text-sm text-[var(--text)] capitalize">{label}</span>
        <span className="text-xs text-[var(--text-subtle)]">{pct(value)}</span>
      </div>
      <div className="h-2 bg-[var(--bg)] border border-[var(--border)] rounded overflow-hidden">
        <div
          className="h-full bg-[var(--accent)] rounded"
          style={{ width: `${(value * 100).toFixed(1)}%` }}
        />
      </div>
      {keywords.length > 0 && (
        <p className="text-xs text-[var(--text-subtle)] mt-0.5">
          {keywords.join(", ")}
        </p>
      )}
    </div>
  );
}

function GambitRow({ text }: { text: string }) {
  return (
    <div className="flex items-center justify-between py-1.5 group">
      <span className="text-sm text-[var(--text)] italic">"{text}"</span>
      <button
        onClick={() => copyToClipboard(text)}
        className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-[var(--bg)]"
        title="Copy"
      >
        <Copy size={13} className="text-[var(--text-subtle)]" />
      </button>
    </div>
  );
}

function ExemplarCard({ exemplar }: { exemplar: Exemplar }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div className="bg-[var(--bg)] border border-[var(--border)] rounded-lg p-3 mb-2">
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <p className="text-xs text-[var(--text-subtle)] mb-1">
            score {fmt(exemplar.score)} · {exemplar.reason}
          </p>
          <p className="text-sm text-[var(--text)] font-mono leading-relaxed break-words">
            {exemplar.chunk_id.slice(0, 8)}…
          </p>
        </div>
        <span className="shrink-0 text-xs text-[var(--text-subtle)] mt-1">
          #{exemplar.chunk_id.slice(0, 6)}
        </span>
      </div>
      <button
        onClick={() => setExpanded((v) => !v)}
        className="mt-1 text-xs text-[var(--accent)] flex items-center gap-0.5"
      >
        {expanded ? (
          <>
            <ChevronUp size={11} /> less
          </>
        ) : (
          <>
            <ChevronDown size={11} /> details
          </>
        )}
      </button>
      {expanded && (
        <p className="mt-1 text-xs text-[var(--text-subtle)] font-mono">
          chunk_id: {exemplar.chunk_id}
        </p>
      )}
    </div>
  );
}

// ─── Section cards ────────────────────────────────────────────────────────────

function VocabularyCard({ lex }: { lex: LexicalProfile }) {
  return (
    <Card title="Vocabulary">
      <div className="flex items-start gap-6 mb-4">
        <div>
          <p className="text-3xl font-bold text-[var(--text)]">
            {fmt(lex.type_token_ratio, 3)}
          </p>
          <p className="text-xs text-[var(--text-subtle)]">Type/Token Ratio</p>
        </div>
        <div className="flex flex-col gap-1">
          <StatChip label="avg word length" value={fmt(lex.avg_word_length)} />
          <StatChip label="level" value={lex.vocabulary_level} />
        </div>
      </div>

      {lex.distinctive_words.length > 0 && (
        <div className="mb-4">
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-2 uppercase tracking-wide">
            Distinctive words
          </p>
          <div>
            {lex.distinctive_words.slice(0, 20).map(({ word, score }) => (
              <WordChip key={word} word={word} score={score} />
            ))}
          </div>
        </div>
      )}

      {lex.characteristic_bigrams.length > 0 && (
        <div className="mb-3">
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-2 uppercase tracking-wide">
            Characteristic phrases
          </p>
          <ul className="space-y-0.5">
            {lex.characteristic_bigrams.slice(0, 8).map(({ phrase, count }) => (
              <li key={phrase} className="flex items-center justify-between">
                <span className="text-sm font-mono text-[var(--text)]">
                  "{phrase}"
                </span>
                <span className="text-xs text-[var(--text-subtle)]">
                  ×{count}
                </span>
              </li>
            ))}
            {lex.characteristic_trigrams.slice(0, 5).map(({ phrase, count }) => (
              <li key={phrase} className="flex items-center justify-between">
                <span className="text-sm font-mono text-[var(--text)]">
                  "{phrase}"
                </span>
                <span className="text-xs text-[var(--text-subtle)]">
                  ×{count}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </Card>
  );
}

function RhythmCard({ syn }: { syn: SyntacticProfile }) {
  const dist = syn.sentence_length_distribution;
  const mix = syn.sentence_type_mix;
  const punc = syn.punctuation_rhythm;

  return (
    <Card title="Rhythm & Structure">
      <div className="mb-4 flex items-center gap-4">
        <div>
          <p className="text-3xl font-bold text-[var(--text)]">
            {fmt(syn.avg_sentence_length, 1)}
          </p>
          <p className="text-xs text-[var(--text-subtle)]">
            avg words / sentence
          </p>
        </div>
        <StatChip
          label="para length"
          value={`${fmt(syn.paragraph_length_avg_sentences, 1)} sent`}
        />
      </div>

      <div className="mb-4">
        <p className="text-xs font-medium text-[var(--text-subtle)] mb-1 uppercase tracking-wide">
          Sentence length
        </p>
        <StackedBar
          segments={[
            {
              label: "short",
              value: dist["short (<10)"],
              color: "bg-sky-400",
            },
            {
              label: "medium",
              value: dist["medium (10-20)"],
              color: "bg-violet-400",
            },
            {
              label: "long",
              value: dist["long (>20)"],
              color: "bg-fuchsia-400",
            },
          ]}
        />
        <div className="flex gap-3 mt-1">
          {[
            { label: "short", v: dist["short (<10)"], color: "bg-sky-400" },
            {
              label: "medium",
              v: dist["medium (10-20)"],
              color: "bg-violet-400",
            },
            { label: "long", v: dist["long (>20)"], color: "bg-fuchsia-400" },
          ].map(({ label, v, color }) => (
            <div key={label} className="flex items-center gap-1">
              <div className={cn("w-2 h-2 rounded-full", color)} />
              <span className="text-xs text-[var(--text-subtle)]">
                {label} {pct(v)}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="mb-4">
        <p className="text-xs font-medium text-[var(--text-subtle)] mb-1 uppercase tracking-wide">
          Sentence types
        </p>
        <StackedBar
          segments={[
            {
              label: "declarative",
              value: mix.declarative,
              color: "bg-emerald-400",
            },
            {
              label: "interrogative",
              value: mix.interrogative,
              color: "bg-amber-400",
            },
            {
              label: "exclamatory",
              value: mix.exclamatory,
              color: "bg-rose-400",
            },
            {
              label: "fragment",
              value: mix.fragment,
              color: "bg-zinc-400",
            },
          ]}
        />
        <div className="flex flex-wrap gap-3 mt-1">
          {Object.entries(mix).map(([k, v]) => (
            <span key={k} className="text-xs text-[var(--text-subtle)]">
              {k} {pct(v)}
            </span>
          ))}
        </div>
      </div>

      <div>
        <p className="text-xs font-medium text-[var(--text-subtle)] mb-2 uppercase tracking-wide">
          Punctuation rhythm
        </p>
        <div className="grid grid-cols-2 gap-2">
          <div className="bg-[var(--bg)] border border-[var(--border)] rounded p-2">
            <p className="text-lg font-semibold text-[var(--text)]">
              {fmt(punc.comma_per_sentence, 1)}
            </p>
            <p className="text-xs text-[var(--text-subtle)]">commas/sentence</p>
          </div>
          <div className="bg-[var(--bg)] border border-[var(--border)] rounded p-2">
            <p className="text-lg font-semibold text-[var(--text)]">
              {fmt(punc.em_dash_per_1000_words, 1)}
            </p>
            <p className="text-xs text-[var(--text-subtle)]">em-dashes/1k</p>
          </div>
          <div className="bg-[var(--bg)] border border-[var(--border)] rounded p-2">
            <p className="text-lg font-semibold text-[var(--text)]">
              {fmt(punc.ellipsis_per_1000_words, 1)}
            </p>
            <p className="text-xs text-[var(--text-subtle)]">ellipses/1k</p>
          </div>
          <div className="bg-[var(--bg)] border border-[var(--border)] rounded p-2">
            <p className="text-lg font-semibold text-[var(--text)]">
              {fmt(punc.semicolon_per_1000_words, 1)}
            </p>
            <p className="text-xs text-[var(--text-subtle)]">semicolons/1k</p>
          </div>
        </div>
      </div>
    </Card>
  );
}

function ThemesCard({ sem }: { sem: SemanticProfile }) {
  return (
    <Card title="Themes">
      {sem.top_topics.length > 0 && (
        <div className="mb-4">
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-3 uppercase tracking-wide">
            Top topics
          </p>
          {sem.top_topics.slice(0, 8).map((t) => (
            <WeightBar
              key={t.label}
              label={t.label}
              value={t.weight}
              keywords={t.keywords}
            />
          ))}
        </div>
      )}

      {sem.recurring_entities.length > 0 && (
        <div className="mb-4">
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-2 uppercase tracking-wide">
            Recurring entities
          </p>
          <div className="flex flex-wrap gap-1">
            {sem.recurring_entities.map(({ entity, count }) => (
              <span
                key={entity}
                className="inline-flex items-center gap-1 bg-[var(--bg)] border border-[var(--border)] rounded-full px-2.5 py-0.5 text-xs"
              >
                <span className="text-[var(--text)]">{entity}</span>
                <span className="text-[var(--text-subtle)]">×{count}</span>
              </span>
            ))}
          </div>
        </div>
      )}

      <div>
        <p className="text-xs font-medium text-[var(--text-subtle)] mb-2 uppercase tracking-wide">
          Sentiment baseline
        </p>
        <div className="flex gap-4">
          <StatChip
            label="polarity"
            value={fmt(sem.sentiment_baseline.polarity, 3)}
          />
          <StatChip
            label="subjectivity"
            value={fmt(sem.sentiment_baseline.subjectivity, 3)}
          />
        </div>
      </div>
    </Card>
  );
}

function VoiceCard({ sty }: { sty: StylisticProfile }) {
  return (
    <Card title="Voice">
      <div className="mb-4">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wide">
            Register
          </span>
          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-[var(--bg)] border border-[var(--border)] text-[var(--text)] capitalize">
            {sty.register}
          </span>
        </div>
        <div className="flex gap-3 mt-2">
          <StatChip
            label="1st person"
            value={pct(sty.first_person_rate)}
          />
          <StatChip
            label="contractions"
            value={pct(sty.contractions_rate)}
          />
        </div>
      </div>

      {sty.opening_gambits.length > 0 && (
        <div className="mb-4">
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-1 uppercase tracking-wide">
            Opening gambits
          </p>
          <div className="divide-y divide-[var(--border)]">
            {sty.opening_gambits.map((g) => (
              <GambitRow key={g} text={g} />
            ))}
          </div>
        </div>
      )}

      {sty.sign_offs.length > 0 && (
        <div>
          <p className="text-xs font-medium text-[var(--text-subtle)] mb-1 uppercase tracking-wide">
            Sign-offs
          </p>
          <div className="divide-y divide-[var(--border)]">
            {sty.sign_offs.map((s) => (
              <GambitRow key={s} text={s} />
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}

function ExemplarsCard({ exemplars }: { exemplars: Exemplar[] }) {
  return (
    <Card title="Exemplars">
      <p className="text-xs text-[var(--text-subtle)] mb-3">
        Most representative chunks from this corpus.
      </p>
      {exemplars.slice(0, 5).map((e) => (
        <ExemplarCard key={e.chunk_id} exemplar={e} />
      ))}
    </Card>
  );
}

// ─── Profile viewer ───────────────────────────────────────────────────────────

function ProfileView({ profile }: { profile: StyleProfile }) {
  const { lex, syn, sem, sty, exemplars } = {
    lex: profile.lexical,
    syn: profile.syntactic,
    sem: profile.semantic,
    sty: profile.stylistic,
    exemplars: profile.exemplars,
  };

  return (
    <div className="space-y-4">
      {lex && <VocabularyCard lex={lex} />}
      {syn && <RhythmCard syn={syn} />}
      {sem && <ThemesCard sem={sem} />}
      {sty && <VoiceCard sty={sty} />}
      {exemplars && exemplars.length > 0 && (
        <ExemplarsCard exemplars={exemplars} />
      )}
    </div>
  );
}

// ─── Era tabs ─────────────────────────────────────────────────────────────────

function EraTabs({
  eras,
  selectedEraId,
  onChange,
}: {
  eras: Era[];
  selectedEraId: string | null;
  onChange: (id: string | null) => void;
}) {
  const tabs = [{ id: null, label: "All" }, ...eras.map((e) => ({ id: e.id, label: e.label }))];
  return (
    <div className="flex gap-1 flex-wrap">
      {tabs.map((t) => (
        <button
          key={t.id ?? "all"}
          onClick={() => onChange(t.id)}
          className={cn(
            "px-3 py-1 rounded text-sm transition-colors",
            selectedEraId === t.id
              ? "bg-[var(--accent)] text-white font-medium"
              : "bg-[var(--bg-elevated)] border border-[var(--border)] text-[var(--text-subtle)] hover:text-[var(--text)]",
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

// ─── Profile status overlays ──────────────────────────────────────────────────

function PendingState({ corpus }: { corpus?: StyleProfile["corpus"] }) {
  return (
    <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-6 text-center">
      <Loader2
        size={24}
        className="mx-auto mb-3 text-[var(--accent)] animate-spin"
      />
      <p className="text-sm font-medium text-[var(--text)] mb-1">
        Computing style profile���
      </p>
      {corpus && (
        <p className="text-xs text-[var(--text-subtle)]">
          {corpus.word_count.toLocaleString()} words · {corpus.chunk_count}{" "}
          chunks
        </p>
      )}
    </div>
  );
}

function InsufficientCorpusState({
  message,
  personaId,
}: {
  message?: string;
  personaId: string;
}) {
  return (
    <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-6 text-center">
      <BookOpen size={28} className="mx-auto mb-3 text-[var(--text-subtle)]" />
      <p className="text-sm font-medium text-[var(--text)] mb-2">
        {message ?? "Not enough content to build a profile yet."}
      </p>
      <Link
        to={`/personas/${personaId}/upload`}
        className="inline-block px-4 py-2 bg-[var(--accent)] text-white text-sm rounded hover:opacity-90 transition-opacity"
      >
        Upload documents
      </Link>
    </div>
  );
}

function EmptyState({ personaId }: { personaId: string }) {
  return (
    <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-8 text-center">
      <BookOpen size={32} className="mx-auto mb-3 text-[var(--text-subtle)]" />
      <p className="text-sm font-medium text-[var(--text)] mb-1">
        No documents uploaded yet
      </p>
      <p className="text-xs text-[var(--text-subtle)] mb-4">
        Upload text files, journals, or audio to build a style profile.
      </p>
      <Link
        to={`/personas/${personaId}/upload`}
        className="inline-block px-4 py-2 bg-[var(--accent)] text-white text-sm rounded hover:opacity-90 transition-opacity"
      >
        Upload documents
      </Link>
    </div>
  );
}

// ─── Main dashboard ───────────────────────────────────────────────────────────

function ProfileSection({
  personaId,
  selectedEraId,
}: {
  personaId: string;
  selectedEraId: string | null;
}) {
  const personaProfileQuery = usePersonaProfile(
    selectedEraId === null ? personaId : "",
  );
  const eraProfileQuery = useEraProfile(
    selectedEraId !== null ? personaId : "",
    selectedEraId,
  );

  const query = selectedEraId === null ? personaProfileQuery : eraProfileQuery;
  const { data: profile, isLoading, isError, error } = query;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12 text-sm text-[var(--text-muted)]">
        <Loader2 size={16} className="animate-spin mr-2" /> Loading��
      </div>
    );
  }

  if (isError) {
    const apiError = error as { status?: number };
    if (apiError?.status === 404) {
      return <EmptyState personaId={personaId} />;
    }
    return (
      <div className="text-sm text-red-500 py-4">
        Failed to load profile.
      </div>
    );
  }

  if (!profile) {
    return <EmptyState personaId={personaId} />;
  }

  if (profile.status === "pending") {
    return <PendingState corpus={profile.corpus} />;
  }

  if (profile.status === "insufficient_corpus") {
    return (
      <InsufficientCorpusState
        message={profile.message}
        personaId={personaId}
      />
    );
  }

  return <ProfileView profile={profile} />;
}

export default function PersonaDashboard() {
  const { id } = useParams<{ id: string }>();
  const personaId = id!;

  const { data: persona, isLoading: personaLoading } = usePersona(personaId);
  const { data: eras = [] } = useErasList(personaId);
  const recompute = useRecomputeProfile(personaId);

  const [selectedEraId, setSelectedEraId] = useState<string | null>(null);

  if (personaLoading) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-muted)]">
        Loading…
      </div>
    );
  }

  if (!persona) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-muted)]">
        Persona not found.
      </div>
    );
  }

  const corpus = undefined; // corpus stats shown per-profile

  return (
    <div className="max-w-2xl mx-auto p-6 pb-16">
      {/* Hero */}
      <div className="mb-6">
        <div className="flex items-center justify-between gap-3 mb-2">
          <div className="flex items-center gap-3 min-w-0">
            <h1 className="text-2xl font-bold text-[var(--text)] truncate">
              {persona.name}
            </h1>
            <RelationBadge relation={persona.relation} />
          </div>
          <button
            onClick={() => recompute.mutate()}
            disabled={recompute.isPending}
            className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 text-xs bg-[var(--bg-elevated)] border border-[var(--border)] rounded hover:bg-[var(--bg)] transition-colors disabled:opacity-50"
            title="Recompute profile"
          >
            <RefreshCw
              size={13}
              className={cn(recompute.isPending && "animate-spin")}
            />
            Recompute
          </button>
        </div>
        {persona.description && (
          <p className="text-sm text-[var(--text-muted)] mb-2">
            {persona.description}
          </p>
        )}
        {/* Era selector */}
        {eras.length > 0 && (
          <div className="mt-3">
            <EraTabs
              eras={eras}
              selectedEraId={selectedEraId}
              onChange={setSelectedEraId}
            />
          </div>
        )}
      </div>

      {/* Profile content */}
      <ProfileSection personaId={personaId} selectedEraId={selectedEraId} />

      {/* Suppress unused variable warning for corpus */}
      {corpus}
    </div>
  );
}
