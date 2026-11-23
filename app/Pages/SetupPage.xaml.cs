using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

/// <summary>First-run setup: welcome, appearance, sources, done. Nothing is enabled without being shown here.</summary>
public sealed partial class SetupPage : Page
{
    private static readonly string[] Steps = { "Welcome", "Appearance", "Sources", "All set" };
    // Enabled on first run because they use public APIs and need no account.
    private static readonly string[] Defaults =
    {
        "native.web", "native.internetarchive", "native.peertube", "native.dailymotion",
    };

    private int _step;
    private readonly StackPanel _body = new() { Spacing = 14 };
    private readonly StackPanel _dots = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center };
    private Button _next = new();
    private Button _back = new();
    private bool _seeded;

    public SetupPage()
    {
        InitializeComponent();
        var card = new StackPanel { Spacing = 22, MaxWidth = 680, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        card.Children.Add(_dots);
        card.Children.Add(_body);

        _back = new Button { Content = "Back", MinWidth = 96 };
        _back.Click += (_, _) => { _step = Math.Max(0, _step - 1); Render(); };
        _next = Ui.Accent("Next", () => _ = NextAsync());
        _next.MinWidth = 120;
        var nav = new Grid();
        nav.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        nav.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        nav.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        nav.Children.Add(_back);
        Grid.SetColumn(_next, 2);
        nav.Children.Add(_next);
        card.Children.Add(nav);
        Root.Children.Add(card);
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        try { await AppState.RefreshAsync(); } catch { }
        Render();
    }

    private async Task NextAsync()
    {
        if (_step < Steps.Length - 1)
        {
            _step++;
            // Seed the recommended sources the first time the Sources step is reached.
            if (_step == 2 && !_seeded) await SeedDefaultsAsync();
            Render();
            return;
        }
        await AppState.SetSettingAsync("setupDone", true);
        App.MainWin?.GoTo("home");
        App.MainWin?.ClearHistory();
    }

    private async Task SeedDefaultsAsync()
    {
        _seeded = true;
        try
        {
            if (!AppState.Current.Providers.Any(p => p.Installed && !p.Custom))
                foreach (var id in Defaults)
                    await Core.CallAsync<JsonElement>("provider.install", new { id });
            await AppState.RefreshAsync();
        }
        catch { /* the Sources step shows the real state */ }
    }

    private void Render()
    {
        _dots.Children.Clear();
        for (int i = 0; i < Steps.Length; i++)
        {
            _dots.Children.Add(new Border
            {
                Width = i == _step ? 28 : 8, Height = 8, CornerRadius = new CornerRadius(4),
                Background = i <= _step ? (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["AccentFillColorDefaultBrush"] : Ui.CardStrong,
            });
        }
        _body.Children.Clear();
        _back.Visibility = _step == 0 ? Visibility.Collapsed : Visibility.Visible;
        _next.Content = _step switch { 0 => "Get started", 3 => "Open CloudStream", _ => "Next" };

        switch (_step)
        {
            case 0: Welcome(); break;
            case 1: Appearance(); break;
            case 2: Sources(); break;
            default: Done(); break;
        }
    }

    private void Welcome()
    {
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.png");
        if (File.Exists(iconPath))
            _body.Children.Add(new Image { Width = 96, Height = 96, HorizontalAlignment = HorizontalAlignment.Center, Source = new BitmapImage(new Uri(iconPath)) });
        var title = Ui.Text("Welcome to CloudStream", 30, true);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        _body.Children.Add(title);
        var intro = Ui.Text(
            "CloudStream plays video from sources you choose. It ships with no content of its own, has no ads or tracking, and keeps your library and history only on this PC.",
            15, false, 0.85);
        intro.TextAlignment = TextAlignment.Center;
        _body.Children.Add(intro);
        var how = Ui.Text("Setup takes under a minute: pick a look, then choose which sources to turn on.", 13, false, 0.65);
        how.TextAlignment = TextAlignment.Center;
        _body.Children.Add(how);
    }

    private void Appearance()
    {
        _body.Children.Add(Ui.Text("Choose a look", 26, true));
        _body.Children.Add(Ui.Text("You can change both later in Settings.", 13, false, 0.65));

        _body.Children.Add(Labeled("Theme", Combo("appearance", new[] { "System", "Light", "Dark" }, "System")));
        _body.Children.Add(Labeled("Window material", Combo("backdrop", new[] { "Mica", "Acrylic" }, "Mica")));
    }

    private static ComboBox Combo(string key, string[] options, string fallback)
    {
        var box = new ComboBox { MinWidth = 220 };
        foreach (var o in options) box.Items.Add(o);
        var cur = AppState.Setting(key, fallback);
        box.SelectedItem = options.Contains(cur) ? cur : fallback;
        box.SelectionChanged += async (_, _) =>
        {
            if (box.SelectedItem is not string v) return;
            await AppState.SetSettingAsync(key, v);
            App.MainWin?.ApplyAppearance();
        };
        return box;
    }

    private static FrameworkElement Labeled(string label, FrameworkElement control)
    {
        var s = new StackPanel { Spacing = 4 };
        s.Children.Add(Ui.Text(label, 13, false, 0.8));
        s.Children.Add(control);
        return s;
    }

    private void Sources()
    {
        _body.Children.Add(Ui.Text("Turn on your sources", 26, true));
        _body.Children.Add(Ui.Text(
            "These use public APIs or official players. Turn off anything you don't want. You can add your own providers for any website later in Extensions.",
            13, false, 0.7));

        foreach (var info in AppState.Current.Providers.Where(p => !p.Custom))
        {
            var d = info.Descriptor;
            var card = new StackPanel { Spacing = 6, Padding = new Thickness(14), Background = Ui.Card, CornerRadius = new CornerRadius(12) };
            var toggle = new ToggleSwitch { Header = d.Name, IsOn = info.Installed };
            card.Children.Add(toggle);
            card.Children.Add(Ui.Text(d.Description, 12, false, 0.7));

            if (d.Id == "native.youtube")
            {
                var key = new PasswordBox
                {
                    PlaceholderText = "YouTube Data API key (optional, free from Google Cloud)",
                    Password = AppState.Setting("youtubeApiKey", ""),
                };
                key.LostFocus += async (_, _) => await AppState.SetSettingAsync("youtubeApiKey", key.Password.Trim());
                card.Children.Add(key);
                card.Children.Add(Ui.Text("Without a key, pasted YouTube links still play through Web links.", 11, false, 0.55));
            }

            var id = d.Id;
            toggle.Toggled += async (_, _) =>
            {
                try
                {
                    await Core.CallAsync<JsonElement>(toggle.IsOn ? "provider.install" : "provider.uninstall", new { id });
                    await AppState.RefreshAsync();
                }
                catch (Exception ex) { card.Children.Add(Ui.Error(ex.Message)); }
            };
            _body.Children.Add(card);
        }
    }

    private void Done()
    {
        var enabled = AppState.Current.Providers.Where(p => p.Installed).Select(p => p.Descriptor.Name).ToList();
        var title = Ui.Text("You're all set", 30, true);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        _body.Children.Add(title);
        var summary = Ui.Text(
            enabled.Count == 0
                ? "No sources are on yet. Open Extensions to turn some on or create your own."
                : "Sources on: " + string.Join(", ", enabled) + ".",
            14, false, 0.85);
        summary.TextAlignment = TextAlignment.Center;
        _body.Children.Add(summary);
        var tip = Ui.Text("Tip: paste a YouTube, Vimeo, Dailymotion or PeerTube link into Search to play it directly.", 13, false, 0.65);
        tip.TextAlignment = TextAlignment.Center;
        _body.Children.Add(tip);
    }
}
