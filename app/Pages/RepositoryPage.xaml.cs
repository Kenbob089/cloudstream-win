using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class RepositoryPage : Page
{
    private RepoDto? _repo;
    private readonly TextBox _search = new() { PlaceholderText = "Filter plugins", MaxWidth = 420, HorizontalAlignment = HorizontalAlignment.Left };
    private readonly ComboBox _status = new() { MinWidth = 180 };
    private readonly StackPanel _list = new() { Spacing = 8 };

    public RepositoryPage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        Root.Children.Clear();
        try
        {
            await AppState.RefreshAsync();
            _repo = AppState.Current.Repositories.FirstOrDefault(r => r.Url == e.Parameter as string);
        }
        catch (Exception ex) { Root.Children.Add(Ui.Error(ex.Message)); return; }

        if (_repo == null)
        {
            Root.Children.Add(Ui.State(Symbol.Important, "Repository not found", "It may have been removed."));
            return;
        }

        var back = new HyperlinkButton { Content = "‹ Extensions" };
        back.Click += (_, _) => App.MainWin?.GoTo("extensions");
        Root.Children.Add(back);
        Root.Children.Add(Ui.Title(_repo.Name));
        if (!string.IsNullOrEmpty(_repo.Description)) Root.Children.Add(Ui.Text(_repo.Description!, 14, false, 0.75));
        Root.Children.Add(new InfoBar
        {
            Severity = InfoBarSeverity.Informational, IsOpen = true, IsClosable = false,
            Title = "Metadata only",
            Message = "These are Android .cs3 plugins. This app never downloads or executes them. An entry becomes usable only if a native provider with the same capabilities exists in this app.",
        });

        _status.Items.Add("All statuses");
        foreach (var l in _repo.Plugins.Select(p => p.Status.Label).Distinct().OrderBy(x => x)) _status.Items.Add(l);
        _status.SelectedIndex = 0;
        _search.TextChanged += (_, _) => Render();
        _status.SelectionChanged += (_, _) => Render();
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        tools.Children.Add(_search);
        tools.Children.Add(_status);
        Root.Children.Add(tools);
        Root.Children.Add(_list);
        Render();
    }

    private void Render()
    {
        if (_repo == null) return;
        _list.Children.Clear();
        var q = _search.Text.Trim();
        var filter = _status.SelectedIndex > 0 ? _status.SelectedItem as string : null;
        var plugins = _repo.Plugins
            .Where(p => filter == null || p.Status.Label == filter)
            .Where(p => q.Length == 0
                || p.Plugin.Name.Contains(q, StringComparison.OrdinalIgnoreCase)
                || p.Plugin.InternalName.Contains(q, StringComparison.OrdinalIgnoreCase)
                || (p.Plugin.Language?.Contains(q, StringComparison.OrdinalIgnoreCase) ?? false))
            .OrderBy(p => p.Plugin.Name, StringComparer.CurrentCultureIgnoreCase)
            .ToList();

        _list.Children.Add(Ui.Text($"{plugins.Count} of {_repo.Plugins.Count} plugins", 12, false, 0.6));
        if (plugins.Count == 0)
        {
            _list.Children.Add(Ui.State(Symbol.Find, "No matches", "Try a different filter."));
            return;
        }

        // Cap rendered rows so very large catalogs stay responsive; filtering narrows them.
        const int cap = 250;
        foreach (var p in plugins.Take(cap)) _list.Children.Add(PluginRow(p));
        if (plugins.Count > cap)
            _list.Children.Add(Ui.Text($"Showing the first {cap}. Use the filter to find others.", 12, false, 0.6));
    }

    private static FrameworkElement PluginRow(PluginWithStatus p)
    {
        var e = p.Plugin;
        var header = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        header.Children.Add(new Border { Width = 32, Height = 32, CornerRadius = new CornerRadius(6), Child = Ui.Img(IconUrl(e), 64) });
        var title = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        title.Children.Add(Ui.Text(e.Name, 14, true, 1, false));
        title.Children.Add(Ui.Text(string.Join(" · ", new[] { e.Version is long v ? "v" + v : null, e.Language }.Where(s => !string.IsNullOrEmpty(s))), 11, false, 0.6, false));
        header.Children.Add(title);

        var body = new StackPanel { Spacing = 8, Padding = new Thickness(0, 6, 0, 6) };
        var chips = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        chips.Children.Add(Ui.Chip(p.Status.Label, true));
        var types = Labels.Plugin(e);
        if (types.Length > 0) chips.Children.Add(Ui.Chip(types));
        body.Children.Add(chips);
        body.Children.Add(Ui.Text(p.Status.Explanation, 12, false, 0.8));
        if (!string.IsNullOrEmpty(e.Description)) body.Children.Add(Ui.Text(e.Description!, 12, false, 0.7));
        if (e.Authors.Count > 0) body.Children.Add(Ui.Text("By " + string.Join(", ", e.Authors), 11, false, 0.6));
        if (e.FileSize is long size) body.Children.Add(Ui.Text($"{size / 1024.0:0.#} KB", 11, false, 0.5, false));

        return new Expander
        {
            Header = header, Content = body, HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
    }

    private static string? IconUrl(PluginEntry e) => e.IconUrl?.Replace("%size%", "64");
}
