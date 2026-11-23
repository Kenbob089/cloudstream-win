using System.Runtime.InteropServices;
using CloudStream.Pages;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Navigation;
using Windows.Graphics;

namespace CloudStream;

public sealed partial class MainWindow : Window
{
    // Smallest size the layout is designed for (device-independent pixels).
    private const int MinWidth = 760, MinHeight = 540;
    // The pane collapses to icons below CompactBelow and re-expands at ExpandAbove (a gap prevents flicker).
    private const double CompactBelow = 980, ExpandAbove = 1040;

    private bool _compact;
    private bool _fullscreen;
    private SubclassProc? _subclass; // kept alive for the lifetime of the window

    public MainWindow()
    {
        InitializeComponent();
        Title = "CloudStream";

        SetUpTitleBar();
        SetUpWindowSize();
        ContentFrame.Navigated += OnNavigated;
        Nav.SelectedItem = Nav.MenuItems[0];
        _ = StartAsync();
    }

    // ------------------------------------------------------------ window chrome

    private void SetUpTitleBar()
    {
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
        var pngPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.png");
        try
        {
            if (File.Exists(iconPath)) AppWindow.SetIcon(iconPath);
            if (File.Exists(pngPath)) TitleIcon.Source = new BitmapImage(new Uri(pngPath));
        }
        catch { /* the icon is cosmetic */ }

        if (AppWindowTitleBar.IsCustomizationSupported())
        {
            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            var tb = AppWindow.TitleBar;
            tb.ButtonBackgroundColor = Colors.Transparent;
            tb.ButtonInactiveBackgroundColor = Colors.Transparent;
        }
        else
        {
            AppTitleBar.Visibility = Visibility.Collapsed;
        }
        if (Content is FrameworkElement root) root.ActualThemeChanged += (_, _) => UpdateCaptionColors();
        UpdateCaptionColors();
    }

    private void UpdateCaptionColors()
    {
        if (!AppWindowTitleBar.IsCustomizationSupported() || Content is not FrameworkElement root) return;
        bool dark = root.ActualTheme != ElementTheme.Light;
        var fg = dark ? Colors.White : Colors.Black;
        var hover = dark ? Windows.UI.Color.FromArgb(40, 255, 255, 255) : Windows.UI.Color.FromArgb(30, 0, 0, 0);
        var tb = AppWindow.TitleBar;
        tb.ButtonForegroundColor = fg;
        tb.ButtonHoverForegroundColor = fg;
        tb.ButtonPressedForegroundColor = fg;
        tb.ButtonInactiveForegroundColor = dark ? Windows.UI.Color.FromArgb(120, 255, 255, 255) : Windows.UI.Color.FromArgb(120, 0, 0, 0);
        tb.ButtonHoverBackgroundColor = hover;
        tb.ButtonPressedBackgroundColor = hover;
    }

    private void SetUpWindowSize()
    {
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        double scale = GetDpiForWindow(hwnd) / 96.0;

        // Initial size: 1280x800, clamped to the work area and centred.
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        int w = Math.Min((int)(1280 * scale), area.Width);
        int h = Math.Min((int)(800 * scale), area.Height);
        AppWindow.MoveAndResize(new RectInt32(area.X + (area.Width - w) / 2, area.Y + (area.Height - h) / 2, w, h));

        // Enforce a minimum size so the layout never has to cope with absurdly small windows.
        _subclass = (hWnd, msg, wParam, lParam, id, data) =>
        {
            if (msg == WM_GETMINMAXINFO)
            {
                var mmi = Marshal.PtrToStructure<MINMAXINFO>(lParam);
                double s = GetDpiForWindow(hWnd) / 96.0;
                mmi.ptMinTrackSize.x = (int)(MinWidth * s);
                mmi.ptMinTrackSize.y = (int)(MinHeight * s);
                Marshal.StructureToPtr(mmi, lParam, false);
                return IntPtr.Zero;
            }
            return DefSubclassProc(hWnd, msg, wParam, lParam);
        };
        SetWindowSubclass(hwnd, _subclass, (UIntPtr)1, UIntPtr.Zero);
    }

    private void RootGrid_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        double w = e.NewSize.Width;
        bool compact = _compact ? w < ExpandAbove : w < CompactBelow;
        if (compact == _compact) return;
        _compact = compact;
        Nav.PaneDisplayMode = compact ? NavigationViewPaneDisplayMode.LeftCompact : NavigationViewPaneDisplayMode.Left;
        if (!compact) Nav.IsPaneOpen = true;
    }

    public bool IsFullscreen => _fullscreen;

    public void SetFullscreen(bool on)
    {
        if (_fullscreen == on) return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Default);
    }

    // ------------------------------------------------------------ startup & appearance

    private async Task StartAsync()
    {
        try
        {
            var snap = await AppState.RefreshAsync();
            ApplyAppearance();
            if (!AppState.Setting("setupDone", false))
            {
                Navigate(typeof(SetupPage));
                return;
            }
            Navigate(typeof(HomePage));
            if (AppState.Setting("autoRefresh", true))
            {
                foreach (var repo in snap.Repositories)
                {
                    try { await Core.CallAsync<System.Text.Json.JsonElement>("repo.refresh", new { url = repo.Url }); }
                    catch { /* failures are recorded by the core and shown on the repository */ }
                }
            }
        }
        catch
        {
            Navigate(typeof(HomePage)); // the page shows its own error
        }
    }

    /// <summary>Applies theme and the Mica / Acrylic window material from settings.</summary>
    public void ApplyAppearance()
    {
        if (Content is FrameworkElement root)
        {
            root.RequestedTheme = AppState.Setting("appearance", "System").ToLowerInvariant() switch
            {
                "light" => ElementTheme.Light,
                "dark" => ElementTheme.Dark,
                _ => ElementTheme.Default,
            };
        }
        SystemBackdrop = AppState.Setting("backdrop", "Mica").ToLowerInvariant() == "acrylic"
            ? new DesktopAcrylicBackdrop()
            : new MicaBackdrop { Kind = Microsoft.UI.Composition.SystemBackdrops.MicaKind.BaseAlt };
        UpdateCaptionColors();
    }

    // ------------------------------------------------------------ navigation

    public void Navigate(Type page, object? parameter = null) =>
        ContentFrame.Navigate(page, parameter, new EntranceNavigationTransitionInfo());

    /// <summary>Drops the back stack (used after first-run setup so Back never returns to it).</summary>
    public void ClearHistory()
    {
        ContentFrame.BackStack.Clear();
        Nav.IsBackEnabled = false;
    }

    public void GoBack()
    {
        if (ContentFrame.CanGoBack) ContentFrame.GoBack();
    }

    /// <summary>Switches tab by tag (used by empty states that point at Extensions).</summary>
    public void GoTo(string tag)
    {
        foreach (var item in Nav.MenuItems.OfType<NavigationViewItem>())
            if ((string)item.Tag == tag) Nav.SelectedItem = item;
        Navigate(PageFor(tag));
    }

    private static Type PageFor(string tag) => tag switch
    {
        "search" => typeof(SearchPage),
        "library" => typeof(LibraryPage),
        "extensions" => typeof(ExtensionsPage),
        _ => typeof(HomePage),
    };

    private void Nav_ItemInvoked(NavigationView sender, NavigationViewItemInvokedEventArgs args)
    {
        if (args.IsSettingsInvoked) Navigate(typeof(SettingsPage));
        else if (args.InvokedItemContainer?.Tag is string tag) Navigate(PageFor(tag));
    }

    private void Nav_BackRequested(NavigationView sender, NavigationViewBackRequestedEventArgs args) => GoBack();

    private void OnNavigated(object sender, NavigationEventArgs e)
    {
        Nav.IsBackEnabled = ContentFrame.CanGoBack && e.SourcePageType != typeof(SetupPage);
        // Full-bleed pages hide the pane: the player, and the first-run setup.
        Nav.IsPaneVisible = e.SourcePageType != typeof(PlayerPage) && e.SourcePageType != typeof(SetupPage);
        // Leaving the player always leaves fullscreen.
        if (e.SourcePageType != typeof(PlayerPage)) SetFullscreen(false);
    }

    // ------------------------------------------------------------ Win32 (minimum window size)

    private const uint WM_GETMINMAXINFO = 0x0024;

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT { public int x; public int y; }

    [StructLayout(LayoutKind.Sequential)]
    private struct MINMAXINFO
    {
        public POINT ptReserved, ptMaxSize, ptMaxPosition, ptMinTrackSize, ptMaxTrackSize;
    }

    private delegate IntPtr SubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, UIntPtr id, UIntPtr data);

    [DllImport("comctl32.dll")]
    private static extern bool SetWindowSubclass(IntPtr hWnd, SubclassProc proc, UIntPtr id, UIntPtr data);

    [DllImport("comctl32.dll")]
    private static extern IntPtr DefSubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hWnd);
}
