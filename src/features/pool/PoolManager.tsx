import { useEffect, useMemo, useState, useCallback, useRef } from "react";
import { keepPreviousData, useQuery, useInfiniteQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { GripVertical, Plus, MessageSquare, RefreshCw, XCircle, X, Trash2, Check, ChevronsUpDown, Tag } from "lucide-react";
import { toast } from "sonner";
import { Card, CardContent } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ChannelEditorDialog } from "@/features/channels/editor";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useApiAdapter } from "@/lib/useApiAdapter";
import { useDirtyPolling } from "../../lib/useDirtyPolling";
import { useTauriEvent } from "@/lib/useTauriEvent";
import { type ApiEntry, type Channel, type PaginatedResult } from "@/types";
import { cn, formatResponseMs, parseResponseMs } from "@/lib/utils";
import { TestChatDialog } from "@/components/proxy/TestChatDialog";
import { getCatalogModel, getCatalogModelExact, getCatalogProviderLogo, formatTokenCount } from "@/lib/modelsCatalog";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

function StatusDot({ state }: { state: string }) {
  return (
    <span
      className={cn("inline-block h-2 w-2 rounded-full", {
        "bg-green-500": state === "closed",
        "bg-red-500": state === "open",
        "bg-gray-400": state === "disabled",
      })}
    />
  );
}

function formatReleaseDate(value?: string) {
  if (!value) return null;
  const compact = value.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compact) return `${compact[1]}-${compact[2]}-${compact[3]}`;
  const monthOnly = value.match(/^(\d{4})-(\d{2})$/);
  if (monthOnly) return `${value}-01`;
  return value;
}

function parseReleaseDateForSort(entry: ApiEntry): number | null {
  const raw = entry.release_date?.trim();
  if (!raw) return null;
  const m1 = raw.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (m1) return new Date(raw).getTime();
  const m2 = raw.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (m2) return new Date(`${m2[1]}-${m2[2]}-${m2[3]}`).getTime();
  const m3 = raw.match(/^(\d{4})-(\d{2})$/);
  if (m3) return new Date(`${raw}-01`).getTime();
  return null;
}


function monthsSinceRelease(value?: string | null): number | null {
  const normalized = formatReleaseDate(value || "");
  if (!normalized) return null;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) return null;
  const now = new Date();
  let months = (now.getFullYear() - date.getFullYear()) * 12 + (now.getMonth() - date.getMonth());
  if (now.getDate() < date.getDate()) months -= 1;
  return Math.max(0, months);
}

function releaseRawScore(value?: string | null): number {
  const months = monthsSinceRelease(value);
  if (months === null) return 40;
  if (months <= 1) return 100;
  if (months >= 12) return 0;
  return Math.max(0, Math.min(100, 100 * (12 - months) / 11));
}

function tierRawScore(entry: ApiEntry): number {
  const text = `${entry.model} ${entry.display_name || ""}`.toLowerCase();
  if (/\b(deprecated|legacy|retired)\b/.test(text)) return 10;
  if (/\b(nano|tiny)\b/.test(text)) return 25;
  if (/\b(mini|lite|small)\b/.test(text)) return 35;
  if (/\b(distill|distilled|int4|awq|gptq)\b/.test(text)) return 35;
  if (/\b(fp8)\b/.test(text)) return 50;
  if (/\b(calibration|eval|benchmark|test)\b/.test(text)) return 55;
  if (/\b(flash|fast|highspeed)\b/.test(text)) return 50;
  if (/\b(turbo)\b/.test(text)) return 62;
  if (/\b(max|ultra|opus)\b/.test(text)) return 95;
  if (/\b(pro|advanced|thinking|reasoning|reasoner)\b/.test(text)) return 88;
  if (/\b(codex|coder|coding|code|kat-dev)\b/.test(text)) return 82;
  if (/\b(plus|large)\b/.test(text)) return 78;
  const size = text.match(/\b(\d+(?:\.\d+)?)(b|t)\b/);
  if (size) {
    const value = Number(size[1]) * (size[2] === "t" ? 1000 : 1);
    if (value >= 200) return 80;
    if (value >= 70) return 75;
  }
  return 70;
}

function parseContextFromMeta(value?: string | null): number {
  const match = (value || "").match(/Context:\s*([0-9]+(?:\.[0-9]+)?)\s*([KkMm])?/);
  if (!match) return 0;
  const amount = Number(match[1]);
  const unit = (match[2] || "").toLowerCase();
  if (unit === "m") return amount * 1_000_000;
  if (unit === "k") return amount * 1_000;
  return amount;
}

function descriptionRawScore(entry: ApiEntry): number {
  const text = `${entry.model} ${entry.display_name || ""} ${entry.model_meta_en || ""} ${entry.model_meta_zh || ""}`.toLowerCase();
  let score = 0;
  if (/\b(reasoning|thinking|reasoner)\b/.test(text) && !/non[-_ ]reasoning/.test(text)) score += 35;
  if (/\b(tool calling|tool_call|function calling|tool use)\b/.test(text)) score += 25;
  if (/\b(struct output|structured output|json schema|json mode)\b/.test(text)) score += 15;
  if (parseContextFromMeta(entry.model_meta_en) >= 128000) score += 15;
  if (/\b(vision|image|multimodal|audio|video|vl)\b/.test(text)) score += 10;
  return Math.min(100, score);
}

function getModelScoreCacheKey(entry: ApiEntry): string {
  return `${entry.model.trim().toLowerCase()}::${entry.release_date || ''}::${entry.display_name || ''}`;
}

function calculateModelRawScore(entry: ApiEntry): number {
  return releaseRawScore(entry.release_date) * 0.4 + tierRawScore(entry) * 0.4 + descriptionRawScore(entry) * 0.2;
}

type CatalogDisplayMeta = {
  logo: string;
  releaseDate: string;
  context: string;
  output: string;
  features: string[];
  modelMetaZh: string;
  modelMetaEn: string;
};

const zhFeatureLabels: Record<string, string> = {
  imageGeneration: "生图",
  imageUnderstanding: "识图",
  audio: "音频",
  video: "视频",
  pdf: "PDF",
  reasoning: "推理",
  interleaved: "思维链",
  toolCall: "工具调用",
  structuredOutput: "结构输出",
  attachment: "附件",
  temperature: "温度",
};
const enFeatureLabels: Record<string, string> = {
  imageGeneration: "Image Gen",
  imageUnderstanding: "Vision",
  audio: "Audio",
  video: "Video",
  pdf: "PDF",
  reasoning: "Reasoning",
  interleaved: "Reasoning Trace",
  toolCall: "Tool Calling",
  structuredOutput: "Struct Output",
  attachment: "Attachment",
  temperature: "Temperature",
};

function buildCatalogDisplayMeta(modelId: string): CatalogDisplayMeta {
  const model = getCatalogModel(modelId);
  if (!model) {
    return {
      logo: getCatalogProviderLogo(modelId),
      releaseDate: "",
      context: "",
      output: "",
      features: [],
      modelMetaZh: "",
      modelMetaEn: "",
    };
  }

  const inputs = model.modalities?.input || [];
  const outputs = model.modalities?.output || [];
  const features: string[] = [];
  if (outputs.includes("image")) features.push("imageGeneration");
  if (inputs.includes("image")) features.push("imageUnderstanding");
  if (inputs.includes("audio") || outputs.includes("audio")) features.push("audio");
  if (inputs.includes("video") || outputs.includes("video")) features.push("video");
  if (inputs.includes("pdf") || outputs.includes("pdf")) features.push("pdf");
  if (model.reasoning) features.push("reasoning");
  if (model.interleaved) features.push("interleaved");
  if (model.tool_call) features.push("toolCall");
  if (model.structured_output) features.push("structuredOutput");
  if (model.attachment) features.push("attachment");
  if (model.temperature) features.push("temperature");

  const releaseDate = formatReleaseDate(model.release_date) || "";
  const context = formatTokenCount(model.limit?.context) || "";
  const output = formatTokenCount(model.limit?.output) || "";
  const buildMeta = (
    labels: Record<string, string>,
    releaseLabel: string,
    contextLabel: string,
    outputLabel: string,
  ) => [
    releaseDate ? `${releaseLabel}: ${releaseDate}` : null,
    ...features.map((f) => labels[f]).filter(Boolean),
    context ? `${contextLabel}: ${context}` : null,
    output ? `${outputLabel}: ${output}` : null,
  ].filter(Boolean).join(" / ");

  return {
    logo: getCatalogProviderLogo(modelId),
    releaseDate,
    context,
    output,
    features,
    modelMetaZh: buildMeta(zhFeatureLabels, "发布", "上下文", "输出"),
    modelMetaEn: buildMeta(enFeatureLabels, "Release", "Context", "Output"),
  };
}

function getEntryDisplayMeta(entry: ApiEntry, catalogMap: Map<string, CatalogDisplayMeta>): CatalogDisplayMeta {
  const fallback = catalogMap.get(entry.model) || buildCatalogDisplayMeta(entry.model);
  return {
    logo: entry.provider_logo || fallback.logo || `${import.meta.env.BASE_URL}logo/custom.svg`,
    releaseDate: entry.release_date || fallback.releaseDate || "",
    context: fallback.context,
    output: fallback.output,
    features: fallback.features,
    modelMetaZh: entry.model_meta_zh || fallback.modelMetaZh || "",
    modelMetaEn: entry.model_meta_en || fallback.modelMetaEn || "",
  };
}

function ModelMetaBlock({ metaZh, metaEn, releaseDate, context, output, features }: {
  metaZh?: string;
  metaEn?: string;
  releaseDate?: string;
  context?: string;
  output?: string;
  features: string[];
}) {
  const { t, i18n } = useTranslation();
  const storedMeta = i18n.language?.startsWith("zh") ? metaZh : metaEn;
  if (storedMeta) return <div className="mt-1 text-xs text-muted-foreground truncate">{storedMeta}</div>;
  if (!releaseDate && features.length === 0 && !context && !output) return null;
  const segments = [
    releaseDate ? `${t("apiPool.modelMeta.releaseDate")}: ${releaseDate}` : null,
    ...features,
    context ? `${t("apiPool.modelMeta.context")}: ${context}` : null,
    output ? `${t("apiPool.modelMeta.output")}: ${output}` : null,
  ].filter(Boolean) as string[];
  if (segments.length === 0) return null;
  return <div className="mt-1 text-xs text-muted-foreground truncate">{segments.join(" / ")}</div>;
}

function getEntryStatus(entry: ApiEntry) {
  const now = Math.floor(Date.now() / 1000);
  if (entry.cooldown_until && entry.cooldown_until > now) return "open";
  if (!entry.enabled) return "disabled";
  return "closed";
}

function formatCooldownRemaining(cooldownUntil: number | null | undefined) {
  if (!cooldownUntil) return null;
  const remaining = Math.max(0, cooldownUntil - Math.floor(Date.now() / 1000));
  if (remaining <= 0) return null;
  return `${Math.ceil(remaining / 60)}m`;
}

function GroupSelector({
  value,
  groups,
  onChange,
}: {
  value: string;
  groups: string[];
  onChange: (group: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");

  const options = useMemo(() => {
    const merged = new Set(["auto", ...groups, value || "auto"]);
    return Array.from(merged).filter(Boolean).sort((a, b) => a.localeCompare(b));
  }, [groups, value]);

  const filtered = draft.trim()
    ? options.filter((item) => item.toLowerCase().includes(draft.trim().toLowerCase()))
    : options;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="flex items-center justify-center rounded p-1 text-muted-foreground hover:bg-muted"
          onClick={(event) => event.stopPropagation()}
        >
          <Tag className="h-4 w-4" />
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-48 p-2" align="start" onClick={(event) => event.stopPropagation()}>
        <Input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={t("apiPool.group.searchPlaceholder")}
          className="mb-2 h-8 text-xs"
        />
        <div className="max-h-40 overflow-y-auto space-y-1">
          {filtered.map((group) => (
            <button
              key={group}
              type="button"
              className={cn(
                "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-accent",
                group === value && "bg-accent"
              )}
              onClick={() => {
                onChange(group);
                setDraft("");
                setOpen(false);
              }}
            >
              <Check className={cn("h-3 w-3", group === value ? "opacity-100" : "opacity-0")} />
              <span className="truncate">{group}</span>
            </button>
          ))}
          {filtered.length === 0 && draft.trim() ? (
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-accent"
              onClick={() => {
                onChange(draft.trim());
                setDraft("");
                setOpen(false);
              }}
            >
              <Check className="h-3 w-3 opacity-0" />
              <span className="truncate">{t("apiPool.group.create", { name: draft.trim() })}</span>
            </button>
          ) : null}
        </div>
      </PopoverContent>
    </Popover>
  );
}

function CardBody({
  entry,
  onTest,
  onDelete,
  onToggleIntent,
  onGroupChange,
  onEditChannel,
  onEditAlias,
  groups,
  testingEntryIds,
  testResult,
  testErrorDetail,
  catalogLogo,
  catalogReleaseDate,
  catalogContext,
  catalogOutput,
  catalogFeatures,
  modelMetaZh,
  modelMetaEn,
}: {
  entry: ApiEntry;
  onTest: (entry: ApiEntry) => void;
  onDelete: (entry: ApiEntry, options?: { shiftKey?: boolean }) => void;
  onToggleIntent: (entry: ApiEntry, enabled: boolean, options: { ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }) => void;
  onGroupChange?: (entry: ApiEntry, group: string) => void;
  onEditChannel?: (entry: ApiEntry) => void;
  onEditAlias?: (entry: ApiEntry) => void;
  groups?: string[];
  testingEntryIds?: Set<string>;
  testResult?: string;
  testErrorDetail?: string;
  catalogLogo: string;
  catalogReleaseDate?: string;
  catalogContext?: string;
  catalogOutput?: string;
  catalogFeatures: string[];
  modelMetaZh?: string;
  modelMetaEn?: string;
}) {
  const { t } = useTranslation();
  const cooldownRemaining = formatCooldownRemaining(entry.cooldown_until);
  return (
    <>
      <div className="h-10 w-10 rounded-md bg-muted/40 border flex items-center justify-center shrink-0 mt-0.5">
        <img src={catalogLogo} alt="provider" className="h-6 w-6 shrink-0" loading="lazy" onError={(e) => {
          e.currentTarget.onerror = null;
          e.currentTarget.src = `${import.meta.env.BASE_URL}logo/custom.svg`;
        }} />
      </div>
      <div className="flex-1 min-w-0 overflow-hidden">
        <div className="flex items-center gap-2 min-w-0">
          <Button variant="link" className="h-auto p-0 font-medium truncate text-foreground max-w-[180px]" onClick={(e) => { e.stopPropagation(); onEditAlias?.(entry); }}>
            {entry.display_name || entry.model}
          </Button>
          <StatusDot state={getEntryStatus(entry)} />
          {onEditChannel && entry.channel_name ? (
            <Button variant="link" className="h-auto p-0 text-foreground font-medium truncate" onClick={(e) => { e.stopPropagation(); onEditChannel(entry); }}>
              {entry.channel_name}
            </Button>
          ) : (
            <span className="font-medium truncate">{entry.channel_name || "—"}</span>
          )}
          {testingEntryIds?.has(entry.id) ? <RefreshCw className="h-3 w-3 animate-spin text-muted-foreground shrink-0" />
            : testResult === "X" ? <XCircle className="h-3 w-3 text-red-500 shrink-0" />
            : testResult ? <span className="text-xs text-green-600 shrink-0">({formatResponseMs(testResult)})</span>
            : entry.response_ms === "X" ? <XCircle className="h-3 w-3 text-red-500 shrink-0" />
            : entry.response_ms ? <span className="text-xs text-green-600 shrink-0">({formatResponseMs(entry.response_ms)})</span>
            : null}
          {cooldownRemaining ? <span className="text-xs text-red-500 shrink-0">{t("apiPool.cooldownInline", { time: cooldownRemaining })}</span> : null}
          {testErrorDetail ? <span className="text-xs text-yellow-600 shrink-0" title={testErrorDetail}>⚠</span> : null}
        </div>
          <div className="mt-1 flex items-center gap-2 min-w-0">
            <ModelMetaBlock
              metaZh={modelMetaZh}
              metaEn={modelMetaEn}
              releaseDate={catalogReleaseDate}
              context={catalogContext}
              output={catalogOutput}
              features={catalogFeatures.map((f) => t(`apiPool.modelMeta.features.${f}`))}
            />
          </div>
      </div>
<div className="flex items-center gap-2">
          <div className="flex items-center gap-0.5">
            {groups && onGroupChange ? (
              <GroupSelector value={entry.group_name || "auto"} groups={groups} onChange={(group) => onGroupChange(entry, group)} />
            ) : null}
            <Button variant="ghost" size="icon" className="h-8 w-8 text-muted-foreground hover:text-foreground touch-none" onClick={() => onTest(entry)}>
              <MessageSquare className="h-4 w-4" />
            </Button>
            <Button variant="ghost" size="icon" className="h-8 w-8 text-muted-foreground hover:text-red-500 touch-none" onClick={(e) => { e.stopPropagation(); onDelete(entry, { shiftKey: e.shiftKey }); }}>
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
          <Switch checked={entry.enabled} onClick={(e) => {
            e.stopPropagation();
            onToggleIntent(entry, !entry.enabled, { ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, metaKey: e.metaKey });
          }} onCheckedChange={() => {}} className="touch-none" />
        </div>
    </>
  );
}

function SortablePoolEntryCard(props: {
  entry: ApiEntry;
  onTest: (entry: ApiEntry) => void;
  onDelete: (entry: ApiEntry, options?: { shiftKey?: boolean }) => void;
  onToggleIntent: (entry: ApiEntry, enabled: boolean, options: { ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }) => void;
  onGroupChange?: (entry: ApiEntry, group: string) => void;
  onEditChannel?: (entry: ApiEntry) => void;
  onEditAlias?: (entry: ApiEntry) => void;
  groups?: string[];
  testingEntryIds?: Set<string>;
  testResult?: string;
  testErrorDetail?: string;
  catalogLogo: string;
  catalogReleaseDate?: string;
  catalogContext?: string;
  catalogOutput?: string;
  catalogFeatures: string[];
  modelMetaZh?: string;
  modelMetaEn?: string;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: props.entry.id });
  const style = { transform: CSS.Transform.toString(transform), transition, zIndex: isDragging ? 10 : undefined, opacity: isDragging ? 0.8 : undefined };
  return (
    <Card ref={setNodeRef} style={style} className={cn("transition-opacity", !props.entry.enabled && "opacity-60")}>
      <CardContent className="flex items-center gap-3 p-4">
        <div {...attributes} {...listeners} className="cursor-pointer text-muted-foreground hover:text-foreground">
          <GripVertical className="h-3.5 w-3.5 shrink-0" />
        </div>
        <CardBody {...props} />
      </CardContent>
    </Card>
  );
}

function PoolEntryCard(props: {
  entry: ApiEntry;
  onTest: (entry: ApiEntry) => void;
  onDelete: (entry: ApiEntry, options?: { shiftKey?: boolean }) => void;
  onToggleIntent: (entry: ApiEntry, enabled: boolean, options: { ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }) => void;
  onGroupChange?: (entry: ApiEntry, group: string) => void;
  onEditChannel?: (entry: ApiEntry) => void;
  onEditAlias?: (entry: ApiEntry) => void;
  groups?: string[];
  testingEntryIds?: Set<string>;
  testResult?: string;
  testErrorDetail?: string;
  catalogLogo: string;
  catalogReleaseDate?: string;
  catalogContext?: string;
  catalogOutput?: string;
  catalogFeatures: string[];
  modelMetaZh?: string;
  modelMetaEn?: string;
}) {
  return (
    <Card className={cn("transition-opacity", !props.entry.enabled && "opacity-60")}>
      <CardContent className="flex items-center gap-3 p-4">
        <CardBody {...props} />
      </CardContent>
    </Card>
  );
}

function AddApiDialog({ open, onOpenChange, channels, channelsLoading, adapter }: {
open: boolean;
onOpenChange: (value: boolean) => void;
channels: Channel[];
channelsLoading: boolean;
adapter: ReturnType<typeof useApiAdapter>;
}) {
const { t } = useTranslation();
const queryClient = useQueryClient();
const [channelId, setChannelId] = useState("");
const [modelName, setModelName] = useState("");
const [displayName, setDisplayName] = useState("");
const channelOptions = channels.filter((c) => c.enabled);

  const createMutation = useMutation({
    mutationFn: () => adapter.pool.create({ channelId, model: modelName, displayName: displayName || undefined, groupName: "auto" }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["entries"] });
      onOpenChange(false);
      setChannelId("");
      setModelName("");
      setDisplayName("");
    },
    onError: (err) => toast.error(`${t("apiPool.addApi")} ${t("common.failed")}: ${err}`),
  });

  return (
    <Dialog open={open} onOpenChange={(value) => {
      if (!value) {
        setChannelId("");
        setModelName("");
        setDisplayName("");
      }
      onOpenChange(value);
    }}>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("apiPool.addModel")}</DialogTitle></DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <div className="text-sm font-medium">{t("apiPool.channel")}</div>
            <Select value={channelId} onValueChange={(value) => {
              setChannelId(value);
              setModelName("");
              setDisplayName("");
            }}>
<SelectTrigger><SelectValue placeholder={t("apiPool.selectChannel")} /></SelectTrigger>
<SelectContent>
{channelsLoading ? (
<SelectItem value="loading" disabled>{t("common.loading")}</SelectItem>
) : channelOptions.length === 0 ? (
<SelectItem value="empty" disabled>{t("apiPool.noEnabledChannels")}</SelectItem>
) : (
channelOptions.map((channel) => <SelectItem key={channel.id} value={channel.id}>{channel.name}</SelectItem>)
)}
</SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium">{t("apiPool.model")}</div>
            <Input value={modelName} onChange={(e) => setModelName(e.target.value)} placeholder={t("apiPool.modelPlaceholder")} />
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium">{t("apiPool.displayName")}</div>
            <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} placeholder={t("apiPool.displayNamePlaceholder")} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t("common.cancel")}</Button>
          <Button onClick={() => createMutation.mutate()} disabled={!channelId || !modelName || createMutation.isPending}>{t("common.add")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function PoolManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const adapter = useApiAdapter();
  const [localOrder, setLocalOrder] = useState<string[] | null>(null);
  const [filterText, setFilterText] = useState("");
  const [debouncedFilter, setDebouncedFilter] = useState("");
  const [filterChannel, setFilterChannel] = useState<string>("all");
  const [showAdd, setShowAdd] = useState(false);
  const [testEntry, setTestEntry] = useState<ApiEntry | null>(null);
  const [testingEntryIds, setTestingEntryIds] = useState<Set<string>>(new Set());
  const [testResults, setTestResults] = useState<Record<string, string>>({});
  const [testErrorDetails, setTestErrorDetails] = useState<Record<string, string>>({});
  const [testProgress, setTestProgress] = useState<{ current: number; total: number } | null>(null);
  const [deleteDialog, setDeleteDialog] = useState<{ entry: ApiEntry; channelMode: boolean } | null>(null);
  const [editAliasEntry, setEditAliasEntry] = useState<ApiEntry | null>(null);
  const [groupFilter, setGroupFilter] = useState<string>("auto");
  const [editingChannel, setEditingChannel] = useState<Channel | null>(null);
  const [channelEditorOpen, setChannelEditorOpen] = useState(false);

  const entriesQueryKey = useMemo(
    () => ["entries", "paginated", groupFilter, filterChannel, debouncedFilter] as const,
    [groupFilter, filterChannel, debouncedFilter],
  );
  const dirtyQueryKeys = useMemo(
    () => [entriesQueryKey, ['channels', 'all'], ['groups']] as const,
    [entriesQueryKey],
  );

  useDirtyPolling('pool', dirtyQueryKeys);

  // 搜索输入 300ms 防抖，避免每次按键都触发后端请求
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedFilter(filterText), 300);
    return () => clearTimeout(timer);
  }, [filterText]);

  // 无限滚动分页加载 entries
  const {
    data: entriesPages,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
  } = useInfiniteQuery({
    queryKey: entriesQueryKey,
    queryFn: ({ pageParam = 1 }) =>
      adapter.pool.listPaginated({
        page: pageParam,
        pageSize: 20,
        groupName: groupFilter !== "all" ? groupFilter : undefined,
        channelId: filterChannel !== "all" ? filterChannel : undefined,
        search: debouncedFilter.trim() || undefined,
      }) as Promise<PaginatedResult<ApiEntry>>,
    getNextPageParam: (lastPage) =>
      lastPage.page * lastPage.page_size < lastPage.total ? lastPage.page + 1 : undefined,
    initialPageParam: 1,
    placeholderData: keepPreviousData,
    staleTime: 0,
  });

  const { data: channels, isLoading: channelsLoading } = useQuery({ queryKey: ["channels", "all"], queryFn: () => adapter.channels.list() as Promise<Channel[]>, staleTime: 2000 });

  // 分组列表从轻量接口单独拉取
  const { data: groupList } = useQuery({
    queryKey: ["groups"],
    queryFn: () => adapter.pool.getGroups() as Promise<string[]>,
    staleTime: 2000,
  });
  const groups = useMemo(() => {
    const vals = groupList ?? [];
    return [...new Set(["auto", ...vals])].filter(Boolean).sort((a, b) => {
      if (a === "auto") return -1;
      if (b === "auto") return 1;
      return a.localeCompare(b);
    });
  }, [groupList]);

  // 所有已加载的 entries 拍平
  const entries = useMemo(() => entriesPages?.pages.flatMap((p) => p.items) ?? [], [entriesPages]);

  // 无限滚动：IntersectionObserver 触发加载更多
  const sentinelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !hasNextPage || isFetchingNextPage) return;
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting) fetchNextPage(); },
      { rootMargin: "200px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  // 过滤条件变化时清除本地排序
  useEffect(() => {
    setLocalOrder(null);
  }, [groupFilter, debouncedFilter, filterChannel]);

   // Desktop-only: Real-time tray reprioritisation via Tauri event.
   // This hook is a no-op on web builds (useTauriEvent returns false).
   // Event: "tray-priority-changed" — triggered when user reorders entries via system tray.
   useTauriEvent("tray-priority-changed", () => {
     queryClient.invalidateQueries({ queryKey: entriesQueryKey });
     queryClient.invalidateQueries({ queryKey: ["settings"] });
   });

   const catalogMap = useMemo(() => {
    const map = new Map<string, CatalogDisplayMeta>();
    for (const entry of entries || []) {
      if (!map.has(entry.model)) {
        const exact = getCatalogModelExact(entry.model);
        if (exact) map.set(entry.model, buildCatalogDisplayMeta(entry.model));
      }
    }
    return map;
  }, [entries]);

   const sorted = useMemo(() => {
     const list = [...(entries || [])];
     const enabled = list.filter((e) => e.enabled).sort((a, b) => a.sort_index - b.sort_index);
     const disabled = list.filter((e) => !e.enabled).sort((a, b) => a.sort_index - b.sort_index);
     return [...enabled, ...disabled];
   }, [entries]);

   const displayEntries = useMemo(() => {
      if (!localOrder) return sorted;
      const ordered = localOrder.map((id) => sorted.find((e) => e.id === id)).filter(Boolean) as ApiEntry[];
      const missing = sorted.filter((entry) => !localOrder.includes(entry.id));
      return [...ordered, ...missing];
    }, [localOrder, sorted]);

  // 过滤条件已进入 queryKey 并由后端分页接口处理，这里只消费当前页结果
  const filteredEntries = useMemo(() => displayEntries, [displayEntries]);
  // 全局排序不依赖于分组/渠道筛选；仅在搜索时不可用
  const canReorder = true; // 允许在搜索过滤状态下拖动排序模型条目

  const reorderMutation = useMutation({
    mutationFn: (orderedIds: string[]) => adapter.pool.reorder(orderedIds),
    onSuccess: () => {
      const scrollY = window.scrollY;
      queryClient.invalidateQueries({ queryKey: entriesQueryKey });
      setLocalOrder(null);
      requestAnimationFrame(() => window.scrollTo(0, scrollY));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => adapter.pool.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: entriesQueryKey });
      setDeleteDialog(null);
    },
  });

  const deleteChannelMutation = useMutation({
    mutationFn: (id: string) => adapter.channels.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: entriesQueryKey });
      queryClient.invalidateQueries({ queryKey: ["channels"] });
      setDeleteDialog(null);
    },
    onError: (err, id) => {
      toast.error(`删除渠道失败: ${err instanceof Error ? err.message : String(err)}`, { id: `delete-channel-${id}` });
    },
  });

  const updateGroupMutation = useMutation({
    mutationFn: ({ id, groupName }: { id: string; groupName: string }) => adapter.pool.updateGroup(id, groupName),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: entriesQueryKey });
    },
    onError: (err) => {
      toast.error(`${t("apiPool.group.updateFailed")}: ${err}`);
    },
  });

const handleGroupChange = useCallback((entry: ApiEntry, group: string) => {
  updateGroupMutation.mutate({ id: entry.id, groupName: group.trim() || "auto" });
}, [updateGroupMutation]);

  const openChannelEditor = useCallback((entry: ApiEntry) => {
    const channel = channels?.find((c) => c.id === entry.channel_id);
    if (!channel) {
      toast.error(`找不到渠道：${entry.channel_id}`);
      return;
    }
    setEditingChannel(channel);
    setChannelEditorOpen(true);
  }, [channels]);

const handleToggleIntent = useCallback(async (entry: ApiEntry, enabled: boolean, options: { ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }) => {
      const hotKey = options.ctrlKey || options.metaKey;
      if (options.shiftKey) {
        const targetEntries = filteredEntries;
        const targetIds = targetEntries.map((e) => e.id);
        // Use batch IPC to avoid N concurrent invoke calls in Tauri
        await adapter.pool.batchToggle(targetIds, enabled);
        requestAnimationFrame(() => queryClient.invalidateQueries({ queryKey: entriesQueryKey }));
        return;
      }

      if (hotKey) {
        (document.activeElement as HTMLElement)?.blur();
        const scrollY = window.scrollY;
        await adapter.pool.toggle(entry.id, true, { pinToTop: true });
        queryClient.invalidateQueries({ queryKey: entriesQueryKey });
        requestAnimationFrame(() => window.scrollTo(0, scrollY));
        return;
      }

      await adapter.pool.toggle(entry.id, enabled);
      requestAnimationFrame(() => queryClient.invalidateQueries({ queryKey: entriesQueryKey }));
    }, [adapter.pool, displayEntries, entriesQueryKey, filteredEntries, localOrder, queryClient]);

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }));



   const handleDragEnd = (event: DragEndEvent) => {
     if (!canReorder) return;
     const { active, over } = event;
     if (!over || active.id === over.id) return;
     const oldIndex = filteredEntries.findIndex((e) => e.id === active.id);
     const newIndex = filteredEntries.findIndex((e) => e.id === over.id);
     if (oldIndex === -1 || newIndex === -1) return;
     const newOrder = arrayMove(filteredEntries, oldIndex, newIndex);
     const newIds = newOrder.map((e) => e.id);
     const remainingIds = displayEntries.filter((entry) => !newIds.includes(entry.id)).map((entry) => entry.id);
     const mergedOrder = [...newIds, ...remainingIds];
     setLocalOrder(mergedOrder);
     reorderMutation.mutate(mergedOrder);
   };

  const testAllEntries = useCallback(async () => {
    if (testProgress) return;
    const firstPage = await adapter.pool.listPaginated({
      page: 1,
      pageSize: 100,
      groupName: groupFilter !== "all" ? groupFilter : undefined,
    }) as PaginatedResult<ApiEntry>;
    const scopedEntries = [...firstPage.items];
    const totalPages = Math.ceil(firstPage.total / firstPage.page_size);
    for (let page = 2; page <= totalPages; page++) {
      const nextPage = await adapter.pool.listPaginated({
        page,
        pageSize: firstPage.page_size,
        groupName: groupFilter !== "all" ? groupFilter : undefined,
      }) as PaginatedResult<ApiEntry>;
      scopedEntries.push(...nextPage.items);
    }
    if (!scopedEntries.length) return;
    const results: Record<string, string> = {};
    const errorDetails: Record<string, string> = {};
    const modelScoreCache = new Map<string, number>();
    let completed = 0;
    const total = scopedEntries.length;
    setTestProgress({ current: 0, total });
    const grouped = new Map<string, ApiEntry[]>();
    for (const entry of scopedEntries) {
      const list = grouped.get(entry.channel_id) || [];
      list.push(entry);
      grouped.set(entry.channel_id, list);
    }
    const testChannel = async (channelEntries: ApiEntry[]) => {
      for (const entry of channelEntries) {
        setTestingEntryIds((prev) => {
          const next = new Set(prev);
          for (const e of channelEntries) next.delete(e.id);
          next.add(entry.id);
          return next;
        });
        try {
          const cacheKey = getModelScoreCacheKey(entry);
          let modelScore = modelScoreCache.get(cacheKey);
          if (modelScore === undefined) {
            modelScore = calculateModelRawScore(entry);
            modelScoreCache.set(cacheKey, modelScore);
          }
          const result = await adapter.pool.testLatency(entry.id, modelScore);

          if (result.latency_ms !== null) {
            results[entry.id] = result.latency_ms.toString();
          } else {
            results[entry.id] = "X";
            // 保存错误详情供前端展示
            if (result.error_detail) {
              errorDetails[entry.id] = result.error_detail;
            }
            await adapter.pool.toggle(entry.id, false);
          }
        } catch (err) {
          results[entry.id] = "X";
          const errMsg = err instanceof Error ? err.message : String(err);
          errorDetails[entry.id] = `exception: ${errMsg}`;
        }
        completed++;
        setTestProgress({ current: completed, total });
        setTestResults({ ...results });
        setTestErrorDetails({ ...errorDetails });

      }
    };
    await Promise.all([...grouped.values()].map(testChannel));
    setTestingEntryIds(new Set());
    setTestResults({});
    setTestErrorDetails({});
    setTestProgress(null);
    await queryClient.invalidateQueries({ queryKey: entriesQueryKey });
  }, [adapter.pool, entriesQueryKey, groupFilter, queryClient, testProgress]);


  return (
    <div className="p-6">
      <div className="flex items-center justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-xl font-semibold">{t("apiPool.title")}</h1>
          <p className="text-sm text-muted-foreground mt-1">{t("apiPool.description")}</p>
        </div>
        <div className="flex items-center gap-3">
          <Button size="sm" variant="outline" className="gap-1.5 min-w-[140px]" onClick={testAllEntries} disabled={!!testProgress}>
            <RefreshCw className={cn("h-4 w-4", testProgress && "animate-spin")} />
            {testProgress ? `${testProgress.current}/${testProgress.total}` : t("apiPool.testAllLatency")}
          </Button>
          <Button size="sm" className="gap-1.5" onClick={() => setShowAdd(true)}>
            <Plus className="h-4 w-4" />
            {t("apiPool.addModel")}
          </Button>
        </div>
      </div>
      <div className="sticky top-0 z-10 bg-background pt-1 pb-1">
        <div className="relative">
          <Input className="flex-1 pr-8" placeholder={t("apiPool.search")} value={filterText} onChange={(e) => setFilterText(e.target.value)} />
          {filterText ? <button type="button" className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground" onClick={() => setFilterText("")}><X className="h-4 w-4" /></button> : null}
        </div>
      </div>
      {groups.length > 0 ? (
        <div className="mt-2 -mx-px w-[calc(100%+2px)] rounded-t-md border border-b-0 bg-background">
          <div className="flex w-full items-center px-0 py-0">
            {groups.map((group, index) => (
              <button
                key={group}
                type="button"
                className={`h-6 flex-1 px-3 text-xs border-b-2 transition-colors ${groupFilter === group ? "border-primary text-foreground" : "border-transparent text-muted-foreground hover:text-foreground"}`}
                onClick={() => {
                  setGroupFilter(group);
                }}
              >
                {group}
              </button>
            ))}
          </div>
        </div>
      ) : null}
      <Card className="rounded-t-none">
        <CardContent className="p-4 pt-4">
          {isLoading ? (
            <div className="space-y-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <div key={i} className="flex items-center gap-3 p-4 border rounded-md">
                  <div className="h-10 w-10 shrink-0 animate-pulse bg-muted rounded" />
                  <div className="flex-1 space-y-2">
                    <div className="h-4 w-1/3 animate-pulse bg-muted rounded" />
                    <div className="h-3 w-1/2 animate-pulse bg-muted rounded" />
                  </div>
                </div>
              ))}
            </div>
          ) : (
            (!entries?.length ? (
              <div className="flex h-48 items-center justify-center text-muted-foreground">{t("apiPool.empty")}</div>
            ) : canReorder ? (
              <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
                <SortableContext items={filteredEntries.map((e) => e.id)} strategy={verticalListSortingStrategy}>
                  <div className="flex flex-col gap-3">
                    {filteredEntries.map((entry) => {
                      const meta = getEntryDisplayMeta(entry, catalogMap);
                      return <SortablePoolEntryCard key={entry.id} entry={entry} onTest={setTestEntry} onDelete={(entry, opts) => { setDeleteDialog({ entry, channelMode: !!opts?.shiftKey }); }} onToggleIntent={handleToggleIntent} onGroupChange={handleGroupChange} onEditChannel={openChannelEditor} onEditAlias={setEditAliasEntry} groups={groups} testingEntryIds={testingEntryIds} testResult={testResults[entry.id]} testErrorDetail={testErrorDetails[entry.id]} catalogLogo={meta.logo} catalogReleaseDate={meta.releaseDate} catalogContext={meta.context} catalogOutput={meta.output} catalogFeatures={meta.features} modelMetaZh={meta.modelMetaZh} modelMetaEn={meta.modelMetaEn} />;
                    })}
                    {/* 无限滚动 sentinel */}
                    <div ref={sentinelRef} className="h-4" />
                    {isFetchingNextPage && (
                      <div className="flex justify-center py-4 text-sm text-muted-foreground">
                        Loading...
                      </div>
                    )}
                  </div>
                </SortableContext>
              </DndContext>
            ) : (
              <div className="flex flex-col gap-3">
                {filteredEntries.map((entry) => {
                  const meta = getEntryDisplayMeta(entry, catalogMap);
                  return <PoolEntryCard key={entry.id} entry={entry} onTest={setTestEntry} onDelete={(entry, opts) => { setDeleteDialog({ entry, channelMode: !!opts?.shiftKey }); }} onToggleIntent={handleToggleIntent} onGroupChange={handleGroupChange} onEditChannel={openChannelEditor} groups={groups} testingEntryIds={testingEntryIds} testResult={testResults[entry.id]} testErrorDetail={testErrorDetails[entry.id]} catalogLogo={meta.logo} catalogReleaseDate={meta.releaseDate} catalogContext={meta.context} catalogOutput={meta.output} catalogFeatures={meta.features} modelMetaZh={meta.modelMetaZh} modelMetaEn={meta.modelMetaEn} />;
                })}
                <div ref={sentinelRef} className="h-4" />
                {isFetchingNextPage && (
                  <div className="flex justify-center py-4 text-sm text-muted-foreground">
                    Loading...
                  </div>
                )}
              </div>
            ))
          )}
        </CardContent>
      </Card>
      <AddApiDialog open={showAdd} onOpenChange={setShowAdd} channels={channels || []} channelsLoading={channelsLoading} adapter={adapter} />

      <Dialog open={!!editAliasEntry} onOpenChange={(open) => { if (!open) setEditAliasEntry(null); }}>
        <DialogContent className="sm:max-w-[400px]">
          <DialogHeader>
            <DialogTitle>{t("apiPool.editAlias")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <div className="text-sm text-muted-foreground">{t("apiPool.channel")}: {editAliasEntry?.channel_name}</div>
              <div className="text-sm text-muted-foreground">{t("apiPool.model")}: {editAliasEntry?.model}</div>
            </div>
            <div className="space-y-2">
              <div className="text-sm font-medium">{t("apiPool.alias")}</div>
              <Input
                defaultValue={editAliasEntry?.display_name || ""}
                id="alias-input"
                placeholder={editAliasEntry?.model || ""}
                onKeyDown={(e) => { if (e.key === "Enter") (e.currentTarget.parentElement?.parentElement?.querySelector(".save-btn") as HTMLButtonElement)?.click(); }}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditAliasEntry(null)}>{t("common.cancel")}</Button>
            <Button className="save-btn" onClick={async () => {
              const input = document.getElementById("alias-input") as HTMLInputElement;
              const newName = input?.value?.trim() || "";
              if (editAliasEntry) {
                try {
                  await adapter.pool.updateDisplayName(editAliasEntry.id, newName);
                  queryClient.invalidateQueries({ queryKey: ["entries"] });
                  setEditAliasEntry(null);
                } catch (err) {
                  toast.error(`${t("common.failed")}: ${err}`);
                }
              }
            }}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <TestChatDialog open={!!testEntry} onOpenChange={(v) => !v && setTestEntry(null)} entry={testEntry} />
      <Dialog open={!!deleteDialog} onOpenChange={(v) => { if (!v) setDeleteDialog(null); }}>
        <DialogContent>
          {deleteDialog?.channelMode ? (
            <>
              <DialogHeader><DialogTitle>删除渠道</DialogTitle></DialogHeader>
              <p className="text-sm text-muted-foreground">确定要删除渠道「{deleteDialog.entry.channel_name || deleteDialog.entry.channel_id}」及其下所有模型吗？此操作不可撤销。</p>
              <DialogFooter>
                <Button variant="outline" onClick={() => setDeleteDialog(null)}>{t("common.cancel")}</Button>
                <Button variant="destructive" disabled={deleteChannelMutation.isPending} onClick={() => { if (deleteDialog) deleteChannelMutation.mutate(deleteDialog.entry.channel_id); }}>删除渠道</Button>
              </DialogFooter>
            </>
          ) : (
            <>
              <DialogHeader><DialogTitle>{t("common.deleteTitle")}</DialogTitle></DialogHeader>
              <p className="text-sm text-muted-foreground">{t("common.deleteWarning")}</p>
              <DialogFooter>
                <Button variant="outline" onClick={() => setDeleteDialog(null)}>{t("common.cancel")}</Button>
                <Button variant="destructive" disabled={deleteMutation.isPending} onClick={() => { if (deleteDialog) deleteMutation.mutate(deleteDialog.entry.id); }}>{t("common.delete")}</Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
      <ChannelEditorDialog
        open={channelEditorOpen}
        channel={editingChannel}
        onOpenChange={(open) => {
          setChannelEditorOpen(open);
          if (!open) setEditingChannel(null);
        }}
        onSaved={async () => {
          setChannelEditorOpen(false);
          setEditingChannel(null);
          await queryClient.invalidateQueries({ queryKey: entriesQueryKey });
          await queryClient.invalidateQueries({ queryKey: ["channels", "all"] });
          await queryClient.invalidateQueries({ queryKey: ["entries"] });
        }}
      />
    </div>
  );
}



