using CloudStream.Pages;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        Title = "CloudStream";
        ContentFrame.Navigated += OnNavigated;
        Nav.SelectedItem = Nav.MenuItems[0];
        Navigate(typeof(HomePage));
        _ = StartAsync();
    }

    private async Task StartAsync()
    {
        try
        {
            var snap = await AppState.RefreshAsync();
            ApplyAppearance();
            if (AppState.Setting("autoRefresh", true))
            {
                foreach (var repo in snap.Repositories)
                {
                    try { await Core.CallAsync<System.Text.Json.JsonElement>("repo.refresh", new { url = repo.Url }); }
                    catch { /* failures are recorded by the core and shown on the repository */ }
                }
            }
        }
        catch { /* surfaced by each page */ }
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
    }

    public void Navigate(Type page, object? parameter = null) =>
        ContentFrame.Navigate(page, parameter, new EntranceNavigationTransitionInfo());

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

    private void Nav_BackRequested(NavigationView sender, NavigationViewBackRequestedEventArgs args)
    {
        if (ContentFrame.CanGoBack) ContentFrame.GoBack();
    }

    private void OnNavigated(object sender, NavigationEventArgs e)
    {
        Nav.IsBackEnabled = ContentFrame.CanGoBack;
        // Full-bleed player: hide the pane while playing.
        Nav.IsPaneVisible = e.SourcePageType != typeof(PlayerPage);
    }
}
