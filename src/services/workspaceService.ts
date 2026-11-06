import { PANE_KINDS, type LayoutNode, type Project, type WorkspaceCollection, type WorkspaceState } from "../types/workspace";

export const WORKSPACE_STORAGE_KEY = "omni-agents-workspace";
const TIPOS_DE_ABA = new Set<string>(PANE_KINDS);

export function emptyWorkspace(): WorkspaceState {
  return {
    version: 1,
    id: "default",
    name: "Workspace",
    projects: [],
    activeProjectId: null,
    updatedAt: new Date().toISOString(),
  };
}

function isWorkspace(value: unknown): value is WorkspaceState {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<WorkspaceState>;
  return (
    candidate.version === 1 &&
    typeof candidate.id === "string" &&
    typeof candidate.name === "string" &&
    Array.isArray(candidate.projects) &&
    candidate.projects.every(isProject) &&
    (candidate.activeProjectId === null ||
      (typeof candidate.activeProjectId === "string" &&
        candidate.projects.some((project) => project.id === candidate.activeProjectId)))
  );
}

function isProject(value: unknown): value is Project {
  if (!value || typeof value !== "object") return false;
  const project = value as Partial<Project>;
  return (
    typeof project.id === "string" &&
    typeof project.name === "string" &&
    typeof project.path === "string" &&
    typeof project.activePaneId === "string" &&
    isLayoutNode(project.layout) &&
    hasPane(project.layout, project.activePaneId)
  );
}

function isLayoutNode(value: unknown): value is LayoutNode {
  if (!value || typeof value !== "object") return false;
  const node = value as Partial<LayoutNode>;
  if (node.type === "pane") {
    return (
      typeof node.id === "string" &&
      Array.isArray(node.tabs) &&
      node.tabs.every(
        (tab) =>
          tab &&
          typeof tab.id === "string" &&
          typeof tab.title === "string" &&
          typeof tab.kind === "string" && TIPOS_DE_ABA.has(tab.kind)
      ) &&
      (node.tabs.length === 0
        ? node.activeTabId === null
        : typeof node.activeTabId === "string" && node.tabs.some((tab) => tab.id === node.activeTabId))
    );
  }
  return (
    node.type === "split" &&
    typeof node.id === "string" &&
    (node.direction === "horizontal" || node.direction === "vertical") &&
    typeof node.ratio === "number" &&
    node.ratio >= 0.2 &&
    node.ratio <= 0.8 &&
    isLayoutNode(node.first) &&
    isLayoutNode(node.second)
  );
}

function hasPane(node: LayoutNode, paneId: string): boolean {
  return node.type === "pane"
    ? node.id === paneId
    : hasPane(node.first, paneId) || hasPane(node.second, paneId);
}

function abaDesconhecida(tab: unknown): boolean {
  const kind = (tab as { kind?: unknown } | null)?.kind;
  return typeof kind === "string" && !TIPOS_DE_ABA.has(kind);
}

function layoutSemAbasDesconhecidas(value: unknown): unknown {
  if (!value || typeof value !== "object") return value;
  const node = value as Record<string, unknown>;
  if (node.type === "split") {
    return { ...node, first: layoutSemAbasDesconhecidas(node.first), second: layoutSemAbasDesconhecidas(node.second) };
  }
  if (node.type !== "pane" || !Array.isArray(node.tabs) || !node.tabs.some(abaDesconhecida)) return value;
  const tabs = node.tabs.filter((tab) => !abaDesconhecida(tab)) as { id?: unknown }[];
  const activeTabId = tabs.some((tab) => tab?.id === node.activeTabId) ? node.activeTabId : tabs[0]?.id ?? null;
  return { ...node, tabs, activeTabId };
}

/** Aba de versão mais nova do app (ou que saiu dele) sai sozinha: o tipo dela não derruba o workspace.
 * Só isso: o resto da estrutura continua sendo conferido por `isWorkspace`. */
function semAbasDesconhecidas(value: unknown): unknown {
  const projects = (value as { projects?: unknown } | null)?.projects;
  if (!Array.isArray(projects)) return value;
  return {
    ...(value as object),
    projects: projects.map((project: unknown) =>
      project && typeof project === "object"
        ? { ...project, layout: layoutSemAbasDesconhecidas((project as { layout?: unknown }).layout) }
        : project
    ),
  };
}

export function loadWorkspace(): WorkspaceState {
  try {
    const raw = localStorage.getItem(WORKSPACE_STORAGE_KEY);
    if (!raw) return emptyWorkspace();
    const parsed = semAbasDesconhecidas(JSON.parse(raw));
    return isWorkspace(parsed) ? parsed : emptyWorkspace();
  } catch {
    return emptyWorkspace();
  }
}

export function saveWorkspace(workspace: WorkspaceState): void {
  localStorage.setItem(WORKSPACE_STORAGE_KEY, JSON.stringify(workspace));
}

export const WORKSPACES_STORAGE_KEY = "omni-agents-workspaces";

function normalizeCollection(raw: unknown): WorkspaceCollection | null {
  if (!raw || typeof raw !== "object") return null;
  const candidate = raw as Partial<WorkspaceCollection>;
  if (candidate.version !== 1 || !Array.isArray(candidate.workspaces)) return null;
  // Um workspace estruturalmente inválido é descartado sozinho, não a coleção inteira —
  // diferente do "tudo ou nada" de isWorkspace, proporcional a uma lista de N itens.
  const workspaces = candidate.workspaces.map(semAbasDesconhecidas).filter(isWorkspace);
  if (workspaces.length === 0) return { version: 1, workspaces: [emptyWorkspace()], activeWorkspaceId: null };
  // Todos escondidos (gravação antiga ou editada à mão): o primeiro volta, senão a lateral fica vazia.
  if (workspaces.every((workspace) => workspace.hidden)) workspaces[0] = { ...workspaces[0], hidden: false };
  const activeWorkspaceId = workspaces.some((workspace) => workspace.id === candidate.activeWorkspaceId && !workspace.hidden)
    ? (candidate.activeWorkspaceId as string)
    : workspaces.find((workspace) => !workspace.hidden)!.id;
  return { version: 1, workspaces, activeWorkspaceId };
}

function migrateLegacyWorkspace(): WorkspaceCollection {
  const legacy = loadWorkspace();
  const workspace =
    legacy.id === "default" && legacy.name === "Workspace" ? { ...legacy, name: "Workspace 1" } : legacy;
  return { version: 1, workspaces: [workspace], activeWorkspaceId: workspace.id };
}

export function loadWorkspaces(): WorkspaceCollection {
  try {
    const raw = localStorage.getItem(WORKSPACES_STORAGE_KEY);
    if (raw) {
      const normalized = normalizeCollection(JSON.parse(raw));
      if (normalized) return normalized;
    }
  } catch {
    // cai para a migração da chave legada abaixo
  }
  return migrateLegacyWorkspace();
}

export function saveWorkspaces(collection: WorkspaceCollection): void {
  localStorage.setItem(WORKSPACES_STORAGE_KEY, JSON.stringify(collection));
}
