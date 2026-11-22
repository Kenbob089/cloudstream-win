using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class SettingsPage : Page
{
    public SettingsPage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        try { await AppState.RefreshAsync(); } catch { }
        Build();
    }

    private static StackPanel Section(string title)
    {
        var s = new StackPanel { Spacing = 10, Padding = new Thickness(14), Background = Ui.Card, CornerRadius = new CornerRadius(12) };
        s.Children.Add(Ui.Header(title));
        return s;
    }

    private static ComboBox Combo(string key, string[] options, string fallback)
    {
        var box = new ComboBox { MinWidth = 200 };
        foreach (var o in options) box.Items.Add(o);
        var cur = AppState.Setting(key, fallback);
        box.SelectedItem = options.Contains(cur) ? cur : fallback;
        box.SelectionChanged += async (_, _) =>
        {
            if (box.SelectedItem is string v) await AppState.SetSettingAsync(key, v);
            if (key is "appearance" or "backdrop") App.MainWin?.ApplyAppearance();
        };
        return box;
    }

    private static ToggleSwitch Toggle(string label, string key, bool fallback)
    {
        var t = new ToggleSwitch { Header = label, IsOn = AppState.Setting(key, fallback) };
        t.Toggled += async (_, _) => await AppState.SetSettingAsync(key, t.IsOn);
        return t;
    }

    private static FrameworkElement Labeled(string label, FrameworkElement control)
    {
        var s = new StackPanel { Spacing = 4 };
        s.Children.Add(Ui.Text(label, 13, false, 0.8));
        s.Children.Add(control);
        return s;
    }

    private void Build()
    {
        Root.Children.Clear();
        Root.Children.Add(Ui.Title("Settings"));

        var look = Section("Appearance");
        look.Children.Add(Labeled("Theme", Combo("appearance", new[] { "System", "Light", "Dark" }, "System")));
        look.Children.Add(Labeled("Window material", Combo("backdrop", new[] { "Mica", "Acrylic" }, "Mica")));
        Root.Children.Add(look);

        var play = Section("Playback");
        play.Children.Add(Labeled("Preferred quality", Combo("quality", new[] { "Auto", "1080p", "720p", "480p" }, "Auto")));
        play.Children.Add(Labeled("Default speed", Combo("speed", new[] { "0.5x", "0.75x", "1x", "1.25x", "1.5x", "2x" }, "1x")));
        play.Children.Add(Toggle("Play next episode automatically", "autoPlayNext", true));
        Root.Children.Add(play);

        var lib = Section("Library & sources");
        lib.Children.Add(Toggle("Show Continue Watching on Home", "continueWatching", true));
        lib.Children.Add(Toggle("Refresh repositories at launch", "autoRefresh", false));
        Root.Children.Add(lib);

        var priv = Section("Privacy & data");
        priv.Children.Add(Ui.Text(
            "CloudStream has no analytics, tracking or ads. Your library, history and settings are stored only on this PC in " + AppState.DataDir + ". The only network requests go to the repositories and providers you add.",
            12, false, 0.75));
        var clearHistory = new Button { Content = "Clear watch history" };
        clearHistory.Click += async (_, _) => await Run("Clear all watch history?", "Resume points and watched marks will be removed.", "Clear", "history.clear");
        var clearCache = new Button { Content = "Clear repository cache" };
        clearCache.Click += async (_, _) => await Run("Clear cached repository data?", "Plugin lists are removed until you refresh a repository.", "Clear", "cache.clear");
        var erase = new Button { Content = "Erase all data" };
        erase.Click += async (_, _) => await Run("Erase all data?", "Bookmarks, history, searches and repositories will be permanently deleted from this PC.", "Erase", "data.erase");
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        buttons.Children.Add(clearHistory); buttons.Children.Add(clearCache); buttons.Children.Add(erase);
        priv.Children.Add(buttons);
        Root.Children.Add(priv);

        var about = Section("About");
        about.Children.Add(Ui.Text("CloudStream for Windows — Rust core with a WinUI 3 interface.", 13));
        about.Children.Add(Ui.Text(
            "An independent implementation inspired by the open-source CloudStream project (GPL-3.0). It does not include or run any Android plugin code, does not bypass DRM or access controls, and ships with no video sources besides the public Internet Archive.",
            12, false, 0.75));
        about.Children.Add(Ui.Text("Provider repository data is treated as untrusted input: it is parsed defensively, fetched over HTTPS only, and never executed.", 12, false, 0.75));
        var link = new HyperlinkButton { Content = "CloudStream project on GitHub", NavigateUri = new Uri("https://github.com/recloudstream/cloudstream") };
        about.Children.Add(link);
        Root.Children.Add(about);
    }

    private async Task Run(string title, string message, string action, string method)
    {
        if (!await Ui.ConfirmAsync(XamlRoot, title, message, action)) return;
        try { await Core.CallAsync<JsonElement>(method); await AppState.RefreshAsync(); }
        catch (Exception ex) { Root.Children.Insert(1, Ui.Error(ex.Message)); }
    }
}
