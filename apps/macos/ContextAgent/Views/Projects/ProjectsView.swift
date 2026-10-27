import SwiftUI
import AppKit

/// The Projects screen: every remembered project as the Welcome screen's
/// recent cards, in a wider grid. The toolbar has the search field at the
/// left (name or path), the filter menu (All / Pinned / Open / Missing, plus
/// the sort order) beside it and "New project" at the right.
struct ProjectsView: View {
    @Environment(AppState.self) private var appState
    @State private var query = ""
    @State private var filter: ProjectFilter = .all
    @State private var sort: ProjectSort = .lastOpened

    private let columns = [GridItem(.adaptive(minimum: 240), spacing: 12, alignment: .top)]

    var body: some View {
        let all = appState.recentProjectCards
        let shown = visible(all)
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 16) {
                toolbar
                if all.isEmpty {
                    EmptyPlaceholder(
                        systemImage: Icons.folder,
                        title: "No projects yet",
                        subtitle: "Projects you open show up here.",
                        actionTitle: "Open a project…"
                    ) {
                        Task { await appState.chooseProject() }
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 48)
                } else {
                    Text(countLabel(shown: shown.count, total: all.count))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                    if shown.isEmpty {
                        noMatches
                    } else {
                        LazyVGrid(columns: columns, alignment: .leading, spacing: 12) {
                            ForEach(shown) { recent in
                                RecentCard(recent: recent, isOpen: isOpen(recent))
                            }
                        }
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 1080, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(Theme.canvas)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Projects")
    }

    // MARK: - Toolbar

    private var toolbar: some View {
        HStack(spacing: 8) {
            SearchField("Search projects", text: $query)
                .frame(width: 280)
            filterMenu
            Spacer(minLength: 8)
            Button {
                Task { await appState.chooseProject(newProject: true) }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: Icons.newChat)
                        .font(.system(size: 12, weight: .semibold))
                    Text("New project")
                        .font(.system(size: 13, weight: .semibold))
                }
                .padding(.horizontal, 4)
            }
            .buttonStyle(.borderedProminent)
            .tint(Theme.accent)
            .disabled(!appState.connection.isReady)
            .help(appState.connection.isReady ? "Choose or create a folder for a new project" : "The backend is not ready")
        }
    }

    private var filterMenu: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        let active = filter != .all
        return Menu {
            Picker("Show", selection: $filter) {
                ForEach(ProjectFilter.allCases) { option in
                    Text(option.title).tag(option)
                }
            }
            .pickerStyle(.inline)
            Picker("Sort by", selection: $sort) {
                ForEach(ProjectSort.allCases) { option in
                    Text(option.title).tag(option)
                }
            }
            .pickerStyle(.inline)
        } label: {
            HStack(spacing: 6) {
                Image(systemName: Icons.filter)
                    .font(.system(size: 12, weight: .medium))
                Text(active ? filter.title : "Filter")
                    .font(Theme.Font.secondary)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
            }
            .foregroundStyle(active ? Theme.accent : Theme.textSecondary)
            .padding(.horizontal, 10)
            .frame(height: 30)
            .background(Theme.surface, in: shape)
            .overlay(shape.strokeBorder(active ? Theme.accent.opacity(0.5) : Theme.border, lineWidth: 1))
            .contentShape(shape)
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Filter and sort projects")
        .accessibilityLabel("Filter: \(filter.title), sorted by \(sort.title)")
    }

    private var noMatches: some View {
        VStack(spacing: 10) {
            Text("No projects match")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            Button("Clear search and filter") {
                query = ""
                filter = .all
            }
            .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 48)
    }

    // MARK: - Filtering

    private func isOpen(_ recent: RecentProject) -> Bool {
        appState.projects.contains { $0.root == recent.path }
    }

    /// Search (name or path) and filter applied to the pinned-first list,
    /// then the chosen sort; pinned stay first either way.
    private func visible(_ recents: [RecentProject]) -> [RecentProject] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let matched = recents.filter { recent in
            let matchesQuery = needle.isEmpty
                || recent.name.localizedCaseInsensitiveContains(needle)
                || recent.abbreviatedPath.localizedCaseInsensitiveContains(needle)
            guard matchesQuery else { return false }
            switch filter {
            case .all: return true
            case .pinned: return recent.pinned
            case .open: return isOpen(recent)
            case .missing: return !recent.exists
            }
        }
        switch sort {
        case .lastOpened:
            return matched
        case .name:
            return matched.sorted { a, b in
                if a.pinned != b.pinned { return a.pinned }
                return a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
        }
    }

    private func countLabel(shown: Int, total: Int) -> String {
        let noun = total == 1 ? "project" : "projects"
        return shown == total ? "\(total) \(noun)" : "\(shown) of \(total) \(noun)"
    }
}

enum ProjectFilter: String, CaseIterable, Identifiable {
    case all, pinned, open, missing
    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: return "All projects"
        case .pinned: return "Pinned"
        case .open: return "Open"
        case .missing: return "Missing"
        }
    }
}

enum ProjectSort: String, CaseIterable, Identifiable {
    case lastOpened, name
    var id: String { rawValue }

    var title: String {
        switch self {
        case .lastOpened: return "Last opened"
        case .name: return "Name"
        }
    }
}

// MARK: - Previews

#Preview("Projects") {
    ProjectsView()
        .frame(width: 1000, height: 700)
        .environment(MainWindowPreview.appState())
}

#Preview("Projects · empty") {
    ProjectsView()
        .frame(width: 1000, height: 700)
        .environment(MainWindowPreview.appState(withProjects: false))
}
