import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Folder, GripVertical, Pencil, Plus, Search, SlidersHorizontal, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { useApiAdapter } from "@/lib/useApiAdapter";
import { cn, formatResponseMs } from "@/lib/utils";
import type { ApiEntry, ModelGroupConfig } from "@/types";

type GroupDraft = {
  name: string;
  description: string;
  enabled: boolean;
};

function groupKey(name: string) {
  return name.trim().toLowerCase();
}

function isAutoGroup(name: string) {
  return groupKey(name) === "auto";
}

function getEntryLabel(entry: ApiEntry) {
  return entry.display_name?.trim() || entry.model;
}

const SORT_PRIORITY_BASE = 100;

function priorityFromSortIndex(sortIndex: number) {
  return SORT_PRIORITY_BASE - Math.round(sortIndex || 0);
}

function sortIndexFromPriority(priority: number) {
  return SORT_PRIORITY_BASE - Math.round(priority);
}

function parsePriorityDraft(value: string | undefined, fallback: number) {
  if (value === undefined || value.trim() === "") return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.round(parsed) : fallback;
}

function sortEntriesForDialog(entries: ApiEntry[]) {
  return [...entries].sort((a, b) => {
    return a.sort_index - b.sort_index;
  });
}

export function GroupManagement() {
  const adapter = useApiAdapter();
  const queryClient = useQueryClient();
  const [groupFormOpen, setGroupFormOpen] = useState(false);
  const [editingGroup, setEditingGroup] = useState<ModelGroupConfig | null>(null);
  const [deleteGroup, setDeleteGroup] = useState<ModelGroupConfig | null>(null);
  const [modelDialogGroup, setModelDialogGroup] = useState<ModelGroupConfig | null>(null);
  const [groupDraft, setGroupDraft] = useState<GroupDraft>({ name: "", description: "", enabled: true });

  const groupsQuery = useQuery({
    queryKey: ["model-groups"],
    queryFn: () => adapter.pool.listModelGroups(),
  });

  const entriesQuery = useQuery({
    queryKey: ["entries", "groups-all"],
    queryFn: () => adapter.pool.list(),
  });

  const groups = groupsQuery.data ?? [];
  const entries = entriesQuery.data ?? [];

  const refreshGroupData = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["model-groups"] }),
      queryClient.invalidateQueries({ queryKey: ["entries"] }),
      queryClient.invalidateQueries({ queryKey: ["groups"] }),
      queryClient.invalidateQueries({ queryKey: ["pool-groups"] }),
      queryClient.invalidateQueries({ queryKey: ["model-group-entry-ids"] }),
    ]);
  };

  const saveGroupMutation = useMutation({
    mutationFn: (draft: GroupDraft) =>
      adapter.pool.upsertModelGroup({
        name: draft.name.trim(),
        description: draft.description.trim(),
        enabled: draft.enabled,
      }),
    onSuccess: async () => {
      setGroupFormOpen(false);
      setEditingGroup(null);
      setGroupDraft({ name: "", description: "", enabled: true });
      await refreshGroupData();
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : String(err));
    },
  });

  const toggleGroupMutation = useMutation({
    mutationFn: ({ name, enabled }: { name: string; enabled: boolean }) =>
      adapter.pool.updateModelGroupEnabled(name, enabled),
    onSuccess: refreshGroupData,
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : String(err));
    },
  });

  const deleteGroupMutation = useMutation({
    mutationFn: (name: string) => adapter.pool.deleteModelGroup(name),
    onSuccess: async () => {
      setDeleteGroup(null);
      await refreshGroupData();
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : String(err));
    },
  });

  const openCreateDialog = () => {
    setGroupFormOpen(true);
    setEditingGroup(null);
    setGroupDraft({ name: "", description: "", enabled: true });
  };

  const openEditDialog = (group: ModelGroupConfig) => {
    setGroupFormOpen(true);
    setEditingGroup(group);
    setGroupDraft({
      name: group.name,
      description: group.description || "",
      enabled: group.enabled,
    });
  };

  const canSaveGroup = groupDraft.name.trim().length > 0 && !saveGroupMutation.isPending;
  const loading = groupsQuery.isLoading || entriesQuery.isLoading;

  return (
    <div className="p-4 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <div className="flex h-9 w-9 items-center justify-center rounded-md border bg-primary/10 text-primary">
              <Folder className="h-5 w-5" />
            </div>
            <div>
              <h1 className="text-xl font-semibold">模型分组</h1>
              <p className="mt-1 text-sm text-muted-foreground">按暴露模型 ID 维护一组上游模型。</p>
            </div>
          </div>
        </div>
        <Button
          size="sm"
          className="gap-1.5"
          onClick={openCreateDialog}
        >
          <Plus className="h-4 w-4" />
          添加分组
        </Button>
      </div>

      <div className="mt-6 overflow-hidden rounded-lg border bg-card">
        <div className="hidden grid-cols-[96px_minmax(180px,1fr)_minmax(220px,1.2fr)_160px_120px] border-b bg-muted/40 px-4 py-3 text-sm font-medium text-muted-foreground md:grid">
          <div>启用</div>
          <div>分组名称</div>
          <div>描述</div>
          <div>模型数</div>
          <div className="text-right">操作</div>
        </div>
        {loading ? (
          <div className="space-y-3 p-4">
            {Array.from({ length: 5 }).map((_, index) => (
              <div key={index} className="h-14 animate-pulse rounded-md bg-muted" />
            ))}
          </div>
        ) : groups.length === 0 ? (
          <div className="flex h-48 items-center justify-center text-sm text-muted-foreground">
            暂无分组
          </div>
        ) : (
          <div className="divide-y">
            {groups.map((group) => {
              const auto = isAutoGroup(group.name);
              const switchDisabled = auto || toggleGroupMutation.isPending;
              return (
                <div
                  key={group.name}
                  className={cn(
                    "grid gap-3 px-4 py-4 md:grid-cols-[96px_minmax(180px,1fr)_minmax(220px,1.2fr)_160px_120px] md:items-center",
                    !group.enabled && "text-muted-foreground opacity-70"
                  )}
                >
                  <div className="flex items-center justify-between gap-3 md:block">
                    <span className="text-xs font-medium text-muted-foreground md:hidden">启用</span>
                    <Switch
                      checked={group.enabled}
                      disabled={switchDisabled}
                      onCheckedChange={(checked) => {
                        toggleGroupMutation.mutate({ name: group.name, enabled: checked === true });
                      }}
                    />
                  </div>
                  <div className="min-w-0">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className="truncate font-medium">{group.name}</span>
                      {group.is_system ? (
                        <span className="shrink-0 rounded border border-primary/20 bg-primary/10 px-1.5 py-0.5 text-xs text-primary">
                          系统预置
                        </span>
                      ) : null}
                    </div>
                  </div>
                  <div className="min-w-0 text-sm text-muted-foreground">
                    <span className="line-clamp-2">{group.description || "—"}</span>
                  </div>
                  <div>
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-9 min-w-[112px] gap-1.5"
                      onClick={() => setModelDialogGroup(group)}
                    >
                      <SlidersHorizontal className="h-4 w-4" />
                      {group.model_count} 个模型
                    </Button>
                  </div>
                  <div className="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-muted-foreground"
                      title="编辑"
                      onClick={() => openEditDialog(group)}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-red-500"
                      title="删除"
                      disabled={auto}
                      onClick={() => setDeleteGroup(group)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <Dialog
        open={groupFormOpen}
        onOpenChange={(open) => {
          setGroupFormOpen(open);
          if (!open) {
            setEditingGroup(null);
            setGroupDraft({ name: "", description: "", enabled: true });
          }
        }}
      >
        <DialogContent className="sm:max-w-[460px]">
          <DialogHeader>
            <DialogTitle>{editingGroup ? "编辑分组" : "添加分组"}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-2">
              <div className="text-sm font-medium">分组名称</div>
              <Input
                value={groupDraft.name}
                disabled={!!editingGroup}
                placeholder="如 code、fast、vision"
                onChange={(event) => setGroupDraft((prev) => ({ ...prev, name: event.target.value }))}
              />
            </div>
            <div className="space-y-2">
              <div className="text-sm font-medium">描述</div>
              <Input
                value={groupDraft.description}
                placeholder="可选"
                onChange={(event) => setGroupDraft((prev) => ({ ...prev, description: event.target.value }))}
              />
            </div>
            <label className="flex items-center justify-between rounded-md border px-3 py-2">
              <span className="text-sm font-medium">启用</span>
              <Switch
                checked={groupDraft.enabled}
                disabled={isAutoGroup(groupDraft.name)}
                onCheckedChange={(checked) => setGroupDraft((prev) => ({ ...prev, enabled: checked === true }))}
              />
            </label>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setGroupFormOpen(false);
                setEditingGroup(null);
                setGroupDraft({ name: "", description: "", enabled: true });
              }}
            >
              取消
            </Button>
            <Button
              disabled={!canSaveGroup}
              onClick={() => saveGroupMutation.mutate(groupDraft)}
            >
              保存
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={!!deleteGroup} onOpenChange={(open) => !open && setDeleteGroup(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除分组</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            确定删除分组「{deleteGroup?.name}」吗？组内模型会移回 auto。
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteGroup(null)}>取消</Button>
            <Button
              variant="destructive"
              disabled={deleteGroupMutation.isPending}
              onClick={() => deleteGroup && deleteGroupMutation.mutate(deleteGroup.name)}
            >
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ModelSelectionDialog
        group={modelDialogGroup}
        entries={entries}
        onClose={() => setModelDialogGroup(null)}
        onSaved={async () => {
          setModelDialogGroup(null);
          await refreshGroupData();
        }}
      />
    </div>
  );
}

function ModelSelectionDialog({
  group,
  entries,
  onClose,
  onSaved,
}: {
  group: ModelGroupConfig | null;
  entries: ApiEntry[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const adapter = useApiAdapter();
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [priorityDrafts, setPriorityDrafts] = useState<Record<string, string>>({});
  const groupName = group?.name ?? "";

  const memberIdsQuery = useQuery({
    queryKey: ["model-group-entry-ids", groupName],
    queryFn: () => adapter.pool.listModelGroupEntryIds(groupName),
    enabled: !!group,
  });

  useEffect(() => {
    if (!group) return;
    setSelectedIds(new Set());
    setPriorityDrafts({});
    setQuery("");
  }, [group?.name]);

  useEffect(() => {
    if (!group || !memberIdsQuery.data) return;
    const memberIds = new Set(memberIdsQuery.data);
    const selectedEntries = entries
      .filter((entry) => memberIds.has(entry.id));
    const next = new Set(selectedEntries.map((entry) => entry.id));
    const nextDrafts: Record<string, string> = {};
    for (const entry of selectedEntries) {
      nextDrafts[entry.id] = String(priorityFromSortIndex(entry.sort_index));
    }
    setSelectedIds(next);
    setPriorityDrafts(nextDrafts);
  }, [entries, group, memberIdsQuery.data]);

  const selectedEntries = useMemo(
    () =>
      entries
        .filter((entry) => selectedIds.has(entry.id))
        .sort((a, b) => {
          const priorityA = parsePriorityDraft(priorityDrafts[a.id], priorityFromSortIndex(a.sort_index));
          const priorityB = parsePriorityDraft(priorityDrafts[b.id], priorityFromSortIndex(b.sort_index));
          if (priorityA !== priorityB) return priorityB - priorityA;
          return a.sort_index - b.sort_index;
        }),
    [entries, priorityDrafts, selectedIds]
  );

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!group) return Promise.resolve();
      const selectedEntryList = entries.filter((entry) => selectedIds.has(entry.id));
      await adapter.pool.replaceModelGroupEntries(group.name, selectedEntryList.map((entry) => entry.id));
      await Promise.all(
        selectedEntryList.map((entry) => {
          const priority = parsePriorityDraft(
            priorityDrafts[entry.id],
            priorityFromSortIndex(entry.sort_index)
          );
          return adapter.pool.updateSortIndex(entry.id, sortIndexFromPriority(priority));
        })
      );
    },
    onSuccess: onSaved,
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : String(err));
    },
  });

  const visibleEntries = useMemo(() => {
    const term = query.trim().toLowerCase();
    const ordered = sortEntriesForDialog(entries);
    if (!term) return ordered;
    return ordered.filter((entry) => {
      const haystack = `${entry.model} ${entry.display_name} ${entry.channel_name || ""}`.toLowerCase();
      return haystack.includes(term);
    });
  }, [entries, query]);

  const toggleEntry = (entry: ApiEntry, checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (checked) next.add(entry.id);
      else next.delete(entry.id);
      return next;
    });
    setPriorityDrafts((prev) => {
      if (checked) {
        if (prev[entry.id] !== undefined) return prev;
        return { ...prev, [entry.id]: String(priorityFromSortIndex(entry.sort_index)) };
      }
      const next = { ...prev };
      delete next[entry.id];
      return next;
    });
  };

  const updatePriorityDraft = (entryId: string, value: string) => {
    setPriorityDrafts((prev) => ({ ...prev, [entryId]: value }));
  };

  return (
    <Dialog open={!!group} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>{group ? `${group.name} - 选择模型` : "选择模型"}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="pl-8"
              value={query}
              placeholder="搜索模型..."
              onChange={(event) => setQuery(event.target.value)}
            />
          </div>

          {selectedEntries.length > 0 ? (
            <div className="max-h-52 overflow-y-auto rounded-md border">
              {selectedEntries.map((entry) => (
                <div key={entry.id} className="grid grid-cols-[24px_minmax(0,1fr)_70px_34px] items-center gap-2 border-b px-3 py-2 last:border-b-0">
                  <GripVertical className="h-4 w-4 text-muted-foreground" />
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{getEntryLabel(entry)}</div>
                    <div className="truncate text-xs text-muted-foreground">{entry.channel_name || entry.channel_id}</div>
                  </div>
                  <Input
                    className="h-8 text-center text-xs"
                    type="number"
                    step={1}
                    title="排序数字，越大越靠前"
                    value={priorityDrafts[entry.id] ?? String(priorityFromSortIndex(entry.sort_index))}
                    onChange={(event) => updatePriorityDraft(entry.id, event.target.value)}
                  />
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 text-muted-foreground hover:text-red-500"
                    onClick={() => toggleEntry(entry, false)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          ) : null}

          <div className="max-h-[48vh] overflow-y-auto rounded-md border">
            {visibleEntries.length === 0 ? (
              <div className="flex h-28 items-center justify-center text-sm text-muted-foreground">
                暂无模型
              </div>
            ) : (
              visibleEntries.map((entry) => {
                const checked = selectedIds.has(entry.id);
                return (
                  <button
                    key={entry.id}
                    type="button"
                    className={cn(
                      "flex w-full items-center gap-3 border-b px-3 py-2 text-left last:border-b-0 hover:bg-accent",
                      checked && "bg-primary/5"
                    )}
                    onClick={() => toggleEntry(entry, !checked)}
                  >
                    <Checkbox
                      checked={checked}
                      onCheckedChange={(value) => toggleEntry(entry, value === true)}
                      onClick={(event) => event.stopPropagation()}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium">{getEntryLabel(entry)}</div>
                      <div className="truncate text-xs text-muted-foreground">
                        {entry.model} · {entry.channel_name || entry.channel_id}
                      </div>
                    </div>
                    {entry.response_ms ? (
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {entry.response_ms === "X" ? "X" : formatResponseMs(entry.response_ms)}
                      </span>
                    ) : null}
                    {checked ? <CheckCircle2 className="h-4 w-4 shrink-0 text-primary" /> : null}
                  </button>
                );
              })
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saveMutation.isPending}>
            取消
          </Button>
          <Button onClick={() => saveMutation.mutate()} disabled={saveMutation.isPending}>
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
