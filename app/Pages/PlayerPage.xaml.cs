using System.Net.NetworkInformation;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Windows.Media.Core;
using Windows.Media.Playback;
using Windows.System;

namespace CloudStream.Pages;

/// <summary>
/// Plays sources resolved by a provider, with CloudStream-style source switching:
/// every playable source is listed in a Sources menu, a failing source falls through to the next one,
/// and embedded players (YouTube, Vimeo, ...) are shown in WebView2. Only https URLs are played.
/// MediaPlayer cannot send custom HTTP headers, so sources that need them are skipped.
/// </summary>
public sealed partial class PlayerPage : Page
{
    private static readonly SolidColorBrush White = new(Microsoft.UI.Colors.White);

    private readonly MediaPlayerElement _view = new()
    {
        AreTransportControlsEnabled = true, Stretch = Stretch.Uniform,
    };
    private WebView2? _web;

    private readonly Grid _bar = new() { Padding = new Thickness(14, 10, 14, 10), VerticalAlignment = VerticalAlignment.Top, ColumnSpacing = 6 };
    private readonly Grid _message = new() { Visibility = Visibility.Collapsed, IsHitTestVisible = true };
    private readonly TextBlock _messageText = new() { TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, Foreground = White, MaxWidth = 480 };
    private readonly Button _retry = Ui.IconButton(Symbol.Refresh, "Retry", () => { });
    private readonly Button _prev = Ui.IconOnly(Symbol.Previous, "Previous episode", () => { });
    private readonly Button _next = Ui.IconOnly(Symbol.Next, "Next episode", () => { });
    private readonly Button _sourcesBtn = new();
    private readonly Button _subsBtn = new();
    private readonly Button _speedBtn = new();
    private readonly Button _fullBtn = Ui.IconOnly(Symbol.FullScreen, "Toggle full screen", () => { });
    private readonly TextBlock _title = new() { Foreground = White, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis };
    private readonly DispatcherTimer _saveTimer = new() { Interval = TimeSpan.FromSeconds(10) };
    private readonly DispatcherTimer _hideTimer = new() { Interval = TimeSpan.FromSeconds(3) };

    private PlayerRequest _req = new();
    private int? _index;
    private List<VideoSource> _sources = new();
    private int _cur;
    private MediaPlayer? _player;
    private MediaPlaybackItem? _item;
    private bool _opened, _failed, _leaving, _webReady;
    private double _resumeAt;
    private readonly DispatcherQueue _ui = DispatcherQueue.GetForCurrentThread();

    private static readonly double[] Speeds = { 0.5, 0.75, 1, 1.25, 1.5, 2 };

    public PlayerPage()
    {
        InitializeComponent();
        _view.TransportControls.IsFullWindowButtonVisible = false;
        _view.TransportControls.IsZoomButtonVisible = false;
        Root.Children.Add(_view);

        // Scrim keeps the white icons legible over bright video.
        Root.Children.Add(new Border
        {
            Height = 72, VerticalAlignment = VerticalAlignment.Top, IsHitTestVisible = false,
            Background = new LinearGradientBrush
            {
                StartPoint = new Windows.Foundation.Point(0, 0), EndPoint = new Windows.Foundation.Point(0, 1),
                GradientStops =
                {
                    new GradientStop { Color = Windows.UI.Color.FromArgb(180, 0, 0, 0), Offset = 0 },
                    new GradientStop { Color = Windows.UI.Color.FromArgb(0, 0, 0, 0), Offset = 1 },
                },
            },
        });

        var close = Ui.IconOnly(Symbol.Back, "Close player", Leave);
        _prev.Click += async (_, _) => await Step(-1);
        _next.Click += async (_, _) => await Step(1);
        _fullBtn.Click += (_, _) => ToggleFullscreen();
        _sourcesBtn.Content = Ui.IconLabel(Symbol.Video, "Sources");
        _subsBtn.Content = Ui.IconLabel(Symbol.ClosedCaption, "Subtitles");
        _speedBtn.Content = Ui.IconLabel(Symbol.Clock, "1x");
        ToolTipService.SetToolTip(_sourcesBtn, "Choose a source");
        ToolTipService.SetToolTip(_subsBtn, "Subtitles");
        ToolTipService.SetToolTip(_speedBtn, "Playback speed");

        int col = 0;
        void Add(UIElement el, bool star = false)
        {
            _bar.ColumnDefinitions.Add(new ColumnDefinition { Width = star ? new GridLength(1, GridUnitType.Star) : GridLength.Auto });
            Grid.SetColumn((FrameworkElement)el, col++);
            _bar.Children.Add(el);
        }
        Add(close); Add(_prev); Add(_next);
        _title.Margin = new Thickness(8, 0, 8, 0);
        Add(_title, true);
        Add(_sourcesBtn); Add(_subsBtn); Add(_speedBtn); Add(_fullBtn);
        Root.Children.Add(_bar);

        var box = new StackPanel { Spacing = 12, VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
        box.Children.Add(_messageText);
        box.Children.Add(_retry);
        _retry.HorizontalAlignment = HorizontalAlignment.Center;
        _message.Children.Add(box);
        Root.Children.Add(_message);

        _retry.Click += async (_, _) => await Load();
        _saveTimer.Tick += async (_, _) => await Save(false);
        _hideTimer.Tick += (_, _) => { _hideTimer.Stop(); if (CanAutoHide) _bar.Visibility = Visibility.Collapsed; };

        Root.PointerMoved += (_, _) => ShowBar();
        KeyDown += OnKey;
        Loaded += (_, _) => Focus(FocusState.Programmatic);
        BuildSpeedMenu();
    }

    private bool CanAutoHide => _message.Visibility != Visibility.Visible
        && _player?.PlaybackSession.PlaybackState == MediaPlaybackState.Playing;

    private void ShowBar()
    {
        _bar.Visibility = Visibility.Visible;
        _hideTimer.Stop();
        _hideTimer.Start();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not PlayerRequest req) return;
        _req = req;
        _index = req.Index;
        _leaving = false;
        NetworkChange.NetworkAvailabilityChanged += OnNetwork;
        await Load();
    }

    protected override async void OnNavigatingFrom(NavigatingCancelEventArgs e)
    {
        base.OnNavigatingFrom(e);
        _leaving = true;
        _hideTimer.Stop();
        NetworkChange.NetworkAvailabilityChanged -= OnNetwork;
        await TearDown();
        if (_web != null) { Root.Children.Remove(_web); _web.Close(); _web = null; _webReady = false; }
    }

    private MediaEpisode? Episode => _index is int i && i >= 0 && i < _req.Episodes.Count ? _req.Episodes[i] : null;
    private VideoSource? Current => _cur >= 0 && _cur < _sources.Count ? _sources[_cur] : null;

    private void ShowMessage(string text, bool retry)
    {
        _messageText.Text = text;
        _retry.Visibility = retry ? Visibility.Visible : Visibility.Collapsed;
        _message.Visibility = Visibility.Visible;
        _bar.Visibility = Visibility.Visible;
    }

    private void ToggleFullscreen()
    {
        var w = App.MainWin;
        if (w == null) return;
        w.SetFullscreen(!w.IsFullscreen);
        _fullBtn.Content = Ui.IconLabel(w.IsFullscreen ? Symbol.BackToWindow : Symbol.FullScreen, null);
    }

    // ---- loading and source selection -------------------------------------------------------

    /// <summary>Loads the sources for the current episode and starts the best one.</summary>
    private async Task Load()
    {
        _message.Visibility = Visibility.Collapsed;
        _failed = false;
        await TearDown(save: false);

        var ep = Episode;
        _title.Text = ep != null ? $"{_req.Item.Title} - {ep.Title}" : _req.Item.Title;
        _prev.IsEnabled = _index > 0;
        _next.IsEnabled = _index is int i && i + 1 < _req.Episodes.Count;
        ShowMessage("Finding sources...", false);
        _retry.Visibility = Visibility.Collapsed;

        List<VideoSource> all;
        try
        {
            all = await Core.CallAsync<List<VideoSource>>("sources", new { providerId = _req.Item.ProviderId, mediaId = _req.Item.Id, episode = ep });
        }
        catch (Exception ex) { ShowMessage(ex.Message, true); return; }
        if (_leaving) return;

        _sources = Order(all);
        _cur = 0;
        BuildSourceMenu();
        if (_sources.Count == 0)
        {
            ShowMessage(all.Any(s => s.Headers.Count > 0)
                ? "These sources require custom HTTP headers, which the Windows player doesn't support."
                : "No playable source was found.", false);
            return;
        }

        var saved = AppState.Current.Progress.FirstOrDefault(p => p.MediaKey == _req.Item.Key && p.EpisodeId == ep?.Id);
        _resumeAt = saved is { Completed: false, Position: > 5 } ? saved.Position : 0;
        await Start();
    }

    /// <summary>https only, no custom headers; direct streams first (preferred quality leading), embeds last.</summary>
    private static List<VideoSource> Order(List<VideoSource> sources)
    {
        var ok = sources.Where(s => s.Headers.Count == 0
            && Uri.TryCreate(s.Url, UriKind.Absolute, out var u) && u.Scheme == Uri.UriSchemeHttps).ToList();
        var want = AppState.Setting("quality", "Auto");
        return ok.Where(s => !s.Embed).OrderByDescending(s => s.Quality.Equals(want, StringComparison.OrdinalIgnoreCase))
            .Concat(ok.Where(s => s.Embed)).ToList();
    }

    private static string Label(VideoSource s)
    {
        var parts = new[] { s.Title, s.Quality }.Where(x => !string.IsNullOrWhiteSpace(x)).Distinct().ToList();
        var text = parts.Count > 0 ? string.Join(" - ", parts) : "Source";
        return s.Embed ? text + " (embedded)" : text;
    }

    private void BuildSourceMenu()
    {
        var menu = new MenuFlyout();
        for (int i = 0; i < _sources.Count; i++)
        {
            int idx = i;
            var item = new ToggleMenuFlyoutItem { Text = Label(_sources[i]), IsChecked = i == _cur };
            item.Click += async (_, _) => await SwitchTo(idx, keepPosition: true);
            menu.Items.Add(item);
        }
        if (_sources.Count == 0) menu.Items.Add(new MenuFlyoutItem { Text = "No sources", IsEnabled = false });
        _sourcesBtn.Flyout = menu;
        _sourcesBtn.Content = Ui.IconLabel(Symbol.Video, _sources.Count > 1 ? $"Sources ({_sources.Count})" : "Sources");
    }

    private async Task SwitchTo(int index, bool keepPosition)
    {
        if (index < 0 || index >= _sources.Count) return;
        if (keepPosition && _player != null && _opened) _resumeAt = _player.PlaybackSession.Position.TotalSeconds;
        _cur = index;
        _failed = false;
        _message.Visibility = Visibility.Collapsed;
        BuildSourceMenu();
        await Start();
    }

    private async Task Start()
    {
        var src = Current;
        if (src == null) return;
        await TearDown(save: false);
        _opened = false;
        BuildSubtitleMenu(src);

        if (src.Embed) { await StartEmbed(src); return; }

        if (_web != null) _web.Visibility = Visibility.Collapsed;
        _view.Visibility = Visibility.Visible;

        var player = new MediaPlayer { AutoPlay = true };
        player.MediaOpened += (_, _) => _ui.TryEnqueue(() => { if (ReferenceEquals(player, _player)) OnOpened(); });
        player.MediaFailed += (_, a) => _ui.TryEnqueue(async () => { if (ReferenceEquals(player, _player)) await OnFailed(a.ErrorMessage); });
        player.MediaEnded += (_, _) => _ui.TryEnqueue(async () => { if (ReferenceEquals(player, _player)) await OnEnded(); });
        player.PlaybackSession.PlaybackStateChanged += (_, _) => _ui.TryEnqueue(() => { if (ReferenceEquals(player, _player)) ShowBar(); });
        _player = player;

        var media = MediaSource.CreateFromUri(new Uri(src.Url));
        foreach (var sub in src.Subtitles.Where(s => Uri.TryCreate(s.Url, UriKind.Absolute, out var u) && u.Scheme == Uri.UriSchemeHttps))
        {
            var label = string.IsNullOrWhiteSpace(sub.Label) ? sub.Language : sub.Label;
            var ts = TimedTextSource.CreateFromUri(new Uri(sub.Url));
            ts.Resolved += (_, a) =>
            {
                if (a.Error != null) return;
                foreach (var t in a.Tracks) t.Label = label;
            };
            media.ExternalTimedTextSources.Add(ts);
        }
        _item = new MediaPlaybackItem(media);
        player.Source = _item;
        _view.SetMediaPlayer(player);
        _saveTimer.Start();
        ShowBar();
    }

    // ---- embedded players --------------------------------------------------------------------

    private async Task StartEmbed(VideoSource src)
    {
        _view.Visibility = Visibility.Collapsed;
        try
        {
            if (_web == null)
            {
                var env = await Microsoft.Web.WebView2.Core.CoreWebView2Environment.CreateWithOptionsAsync(
                    null, Path.Combine(AppState.DataDir, "WebView2"),
                    new Microsoft.Web.WebView2.Core.CoreWebView2EnvironmentOptions("--autoplay-policy=no-user-gesture-required"));
                _web = new WebView2 { HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Stretch };
                Root.Children.Insert(1, _web);
                await _web.EnsureCoreWebView2Async(env);
                var core = _web.CoreWebView2;
                // The embed page is served from a local folder under a virtual host so the iframe gets a valid Referer.
                core.SetVirtualHostNameToFolderMapping("cloudstream.local",
                    Path.Combine(AppContext.BaseDirectory, "Assets", "web"),
                    Microsoft.Web.WebView2.Core.CoreWebView2HostResourceAccessKind.Allow);
                core.NewWindowRequested += (_, a) => a.Handled = true;
                core.NavigationStarting += (_, a) =>
                {
                    if (!Uri.TryCreate(a.Uri, UriKind.Absolute, out var u) || u.Host != "cloudstream.local") a.Cancel = !a.Uri.Equals("about:blank", StringComparison.OrdinalIgnoreCase);
                };
                core.Settings.AreDefaultContextMenusEnabled = false;
                core.Settings.IsStatusBarEnabled = false;
                _webReady = true;
            }
            if (_leaving) return;
            _web.Visibility = Visibility.Visible;
            _web.Source = new Uri("https://cloudstream.local/embed.html?src=" + Uri.EscapeDataString(src.Url));
            _message.Visibility = Visibility.Collapsed;
            _opened = true;
            await RecordEmbedHistory();
        }
        catch (Exception ex)
        {
            await AdvanceOrFail("The embedded player couldn't start: " + ex.Message);
        }
    }

    /// <summary>Embedded players expose no position, so only a history entry (below the resume threshold) is recorded.</summary>
    private async Task RecordEmbedHistory()
    {
        try
        {
            await Core.CallAsync<System.Text.Json.JsonElement>("progress.save",
                new { item = _req.Item, episode = Episode, position = 1.0, duration = 100.0, completed = false });
        }
        catch { /* best-effort */ }
    }

    // ---- subtitles and speed ------------------------------------------------------------------

    private void BuildSubtitleMenu(VideoSource src)
    {
        var menu = new MenuFlyout();
        var off = new ToggleMenuFlyoutItem { Text = "Off", IsChecked = true };
        off.Click += (_, _) => SetSubtitle(-1, menu);
        menu.Items.Add(off);
        for (int i = 0; i < src.Subtitles.Count; i++)
        {
            int idx = i;
            var s = src.Subtitles[i];
            var item = new ToggleMenuFlyoutItem { Text = string.IsNullOrWhiteSpace(s.Label) ? (string.IsNullOrWhiteSpace(s.Language) ? "Track " + (i + 1) : s.Language) : s.Label };
            item.Click += (_, _) => SetSubtitle(idx, menu);
            menu.Items.Add(item);
        }
        _subsBtn.Flyout = menu;
        _subsBtn.Visibility = !src.Embed && src.Subtitles.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void SetSubtitle(int index, MenuFlyout menu)
    {
        for (int i = 0; i < menu.Items.Count; i++)
            if (menu.Items[i] is ToggleMenuFlyoutItem t) t.IsChecked = i == index + 1;
        var tracks = _item?.TimedMetadataTracks;
        if (tracks == null) return;
        for (int i = 0; i < tracks.Count; i++)
            tracks.SetPresentationMode((uint)i, i == index
                ? TimedMetadataTrackPresentationMode.PlatterOnly
                : TimedMetadataTrackPresentationMode.Disabled);
    }

    private void BuildSpeedMenu()
    {
        var menu = new MenuFlyout();
        foreach (var s in Speeds)
        {
            var rate = s;
            var item = new MenuFlyoutItem { Text = rate.ToString("0.##", System.Globalization.CultureInfo.InvariantCulture) + "x" };
            item.Click += (_, _) => SetSpeed(rate);
            menu.Items.Add(item);
        }
        _speedBtn.Flyout = menu;
    }

    private void SetSpeed(double rate)
    {
        if (_player != null) _player.PlaybackSession.PlaybackRate = rate;
        _speedBtn.Content = Ui.IconLabel(Symbol.Clock, rate.ToString("0.##", System.Globalization.CultureInfo.InvariantCulture) + "x");
    }

    // ---- player events ------------------------------------------------------------------------

    private void OnOpened()
    {
        if (_player == null) return;
        _opened = true;
        _message.Visibility = Visibility.Collapsed;
        var speed = AppState.Setting("speed", "1x").TrimEnd('x');
        if (double.TryParse(speed, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var rate) && rate > 0)
            SetSpeed(rate);
        if (_resumeAt > 0 && _player.PlaybackSession.NaturalDuration.TotalSeconds > _resumeAt + 5)
            _player.PlaybackSession.Position = TimeSpan.FromSeconds(_resumeAt);
        _resumeAt = 0;
    }

    private async Task OnFailed(string message)
    {
        if (_opened)
        {
            // Failed mid-playback: offer retry, and resume from here when the network returns.
            _failed = true;
            ShowMessage("Playback stopped: " + (string.IsNullOrWhiteSpace(message) ? "the stream was interrupted." : message), true);
            return;
        }
        await AdvanceOrFail(string.IsNullOrWhiteSpace(message) ? "the stream couldn't be opened." : message);
    }

    /// <summary>Falls through to the next source, or shows an error when none are left.</summary>
    private async Task AdvanceOrFail(string reason)
    {
        if (_leaving) return;
        if (_cur + 1 < _sources.Count)
        {
            ShowMessage($"Source failed. Trying next source ({_cur + 2} of {_sources.Count})...", false);
            _retry.Visibility = Visibility.Collapsed;
            await SwitchTo(_cur + 1, keepPosition: false);
            return;
        }
        _failed = true;
        ShowMessage("Playback failed on every source: " + reason, true);
    }

    private async Task OnEnded()
    {
        await Save(true);
        if (AppState.Setting("autoPlayNext", true) && _next.IsEnabled) await Step(1);
    }

    private async Task Step(int delta)
    {
        if (_index is not int i) return;
        int n = i + delta;
        if (n < 0 || n >= _req.Episodes.Count) return;
        await Save(false);
        _index = n;
        await Load();
    }

    private void OnNetwork(object? sender, NetworkAvailabilityEventArgs e)
    {
        // Retry automatically when connectivity returns after a failure.
        if (e.IsAvailable && _failed && !_leaving) _ui.TryEnqueue(async () =>
        {
            if (_sources.Count > 0 && _player != null && _opened) { _resumeAt = _player.PlaybackSession.Position.TotalSeconds; await SwitchTo(_cur, false); }
            else await Load();
        });
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Space when _player != null:
                if (_player.PlaybackSession.PlaybackState == MediaPlaybackState.Playing) _player.Pause(); else _player.Play();
                break;
            case VirtualKey.Left when _player != null: Seek(-10); break;
            case VirtualKey.Right when _player != null: Seek(10); break;
            case VirtualKey.M when _player != null: _player.IsMuted = !_player.IsMuted; break;
            case VirtualKey.F: ToggleFullscreen(); break;
            case VirtualKey.N: _ = Step(1); break;
            case VirtualKey.P: _ = Step(-1); break;
            case VirtualKey.Escape:
                if (App.MainWin?.IsFullscreen == true) ToggleFullscreen(); else Leave();
                break;
            default: return;
        }
        e.Handled = true;
        ShowBar();
    }

    private void Seek(double seconds)
    {
        if (_player == null) return;
        var s = _player.PlaybackSession;
        var target = s.Position + TimeSpan.FromSeconds(seconds);
        if (target < TimeSpan.Zero) target = TimeSpan.Zero;
        if (s.NaturalDuration > TimeSpan.Zero && target > s.NaturalDuration) target = s.NaturalDuration;
        s.Position = target;
    }

    // ---- progress and teardown ----------------------------------------------------------------

    private async Task Save(bool completed)
    {
        if (_player == null || !_opened) return;
        var s = _player.PlaybackSession;
        double pos = s.Position.TotalSeconds, dur = s.NaturalDuration.TotalSeconds;
        if (dur <= 0) return;
        try
        {
            await Core.CallAsync<System.Text.Json.JsonElement>("progress.save",
                new { item = _req.Item, episode = Episode, position = pos, duration = dur, completed });
        }
        catch { /* progress is best-effort; never interrupt playback */ }
    }

    private async Task TearDown(bool save = true)
    {
        _saveTimer.Stop();
        if (save) await Save(false);
        if (_web != null && _webReady) { try { _web.Source = new Uri("about:blank"); } catch { } }
        if (_player != null)
        {
            var p = _player;
            _player = null;
            _item = null;
            _view.SetMediaPlayer(null);
            p.Pause();
            p.Dispose();
        }
    }

    private void Leave()
    {
        _leaving = true;
        NetworkChange.NetworkAvailabilityChanged -= OnNetwork;
        App.MainWin?.GoBack();
    }
}
