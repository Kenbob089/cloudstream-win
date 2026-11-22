using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.UI;

namespace CloudStream;

/// <summary>Shared UI building blocks. Translucent brushes let the Mica/Acrylic backdrop show through.</summary>
public static class Ui
{
    public static readonly Brush Card = new SolidColorBrush(Color.FromArgb(34, 128, 128, 128));
    public static readonly Brush CardStrong = new SolidColorBrush(Color.FromArgb(60, 128, 128, 128));
    private static readonly Brush Clear = new SolidColorBrush(Colors.Transparent);

    public static TextBlock Text(string text, double size = 14, bool bold = false, double opacity = 1, bool wrap = true,
        int maxLines = 0) => new()
    {
        Text = text,
        FontSize = size,
        FontWeight = bold ? FontWeights.SemiBold : FontWeights.Normal,
        Opacity = opacity,
        TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap,
        TextTrimming = TextTrimming.CharacterEllipsis,
        MaxLines = maxLines,
    };

    public static TextBlock Title(string text) => Text(text, 32, true);
    public static TextBlock Header(string text)
    {
        var t = Text(text, 20, true);
        AutomationProperties.SetHeadingLevel(t, AutomationHeadingLevel.Level2);
        return t;
    }

    /// <summary>https-only image with decode-size limiting so full-resolution art is never kept in memory.</summary>
    public static Image Img(string? url, int decodeWidth)
    {
        var img = new Image { Stretch = Stretch.UniformToFill };
        if (Uri.TryCreate(url, UriKind.Absolute, out var uri) && uri.Scheme == Uri.UriSchemeHttps)
            img.Source = new BitmapImage { DecodePixelWidth = decodeWidth, UriSource = uri };
        return img;
    }

    public static Button Plain(UIElement content, Action onClick, string? accessibleName = null)
    {
        var b = new Button
        {
            Content = content,
            Padding = new Thickness(0),
            Background = Clear,
            BorderThickness = new Thickness(0),
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
        b.Click += (_, _) => onClick();
        if (accessibleName != null) AutomationProperties.SetName(b, accessibleName);
        return b;
    }

    public static Button Poster(MediaItem item, string? provider, double progress, Action onClick, double width = 140)
    {
        var art = new Grid { Width = width, Height = width * 1.5 };
        art.Children.Add(Img(item.PosterUrl, (int)(width * 2)));
        if (progress > 0)
            art.Children.Add(new ProgressBar
            {
                Value = progress * 100, Maximum = 100, VerticalAlignment = VerticalAlignment.Bottom,
                Margin = new Thickness(8),
            });
        var frame = new Border { Child = art, CornerRadius = new CornerRadius(12), Background = Card, Width = width, Height = width * 1.5 };

        var sub = string.Join(" · ", new[] { item.ReleaseYear?.ToString(), provider }.Where(s => !string.IsNullOrEmpty(s)));
        var stack = new StackPanel { Width = width, Spacing = 4 };
        stack.Children.Add(frame);
        stack.Children.Add(Text(item.Title, 13, true, 1, true, 2));
        if (sub.Length > 0) stack.Children.Add(Text(sub, 12, false, 0.65, false));
        return Plain(stack, onClick, item.Title);
    }

    public static FrameworkElement Shelf(string title, IEnumerable<MediaItem> items, Func<MediaItem, double>? progress = null)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 14 };
        foreach (var item in items)
            row.Children.Add(Poster(item, AppState.ProviderName(item.ProviderId), progress?.Invoke(item) ?? 0, () => Open(item)));
        var host = new StackPanel { Spacing = 10 };
        host.Children.Add(Header(title));
        host.Children.Add(new ScrollViewer
        {
            Content = row,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            HorizontalScrollMode = ScrollMode.Enabled,
            VerticalScrollMode = ScrollMode.Disabled,
        });
        return host;
    }

    public static FrameworkElement PosterGrid(IEnumerable<UIElement> posters) => new ItemsRepeater
    {
        ItemsSource = posters.ToList(),
        Layout = new UniformGridLayout { MinItemWidth = 150, MinItemHeight = 270, MinRowSpacing = 14, MinColumnSpacing = 14 },
    };

    public static void Open(MediaItem item) => App.MainWin?.Navigate(typeof(Pages.DetailsPage), item);

    public static FrameworkElement State(Symbol symbol, string title, string message, string? action = null, Action? onAction = null)
    {
        var s = new StackPanel
        {
            Spacing = 10, HorizontalAlignment = HorizontalAlignment.Center, Padding = new Thickness(24, 48, 24, 24),
            MaxWidth = 420,
        };
        s.Children.Add(new SymbolIcon(symbol) { RenderTransform = new ScaleTransform { ScaleX = 2, ScaleY = 2 }, Opacity = 0.6, Margin = new Thickness(0, 0, 0, 12) });
        s.Children.Add(new TextBlock { Text = title, FontSize = 20, FontWeight = FontWeights.SemiBold, HorizontalAlignment = HorizontalAlignment.Center });
        s.Children.Add(new TextBlock { Text = message, Opacity = 0.7, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center });
        if (action != null && onAction != null)
        {
            var b = Accent(action, onAction);
            b.HorizontalAlignment = HorizontalAlignment.Center;
            s.Children.Add(b);
        }
        return s;
    }

    public static Button Accent(string text, Action onClick)
    {
        var b = new Button { Content = text };
        if (Application.Current.Resources.TryGetValue("AccentButtonStyle", out var style) && style is Style st) b.Style = st;
        b.Click += (_, _) => onClick();
        return b;
    }

    public static Border Chip(string text, bool emphasized = false) => new()
    {
        Child = Text(text, 12, true, 1, false),
        Padding = new Thickness(10, 4, 10, 4),
        CornerRadius = new CornerRadius(12),
        Background = emphasized ? CardStrong : Card,
    };

    public static InfoBar Error(string message) => new()
    {
        Severity = InfoBarSeverity.Error, Message = message, IsOpen = true, IsClosable = true,
    };

    public static async Task<bool> ConfirmAsync(XamlRoot root, string title, string message, string primary)
    {
        var dlg = new ContentDialog
        {
            XamlRoot = root, Title = title, Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap },
            PrimaryButtonText = primary, CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        return await dlg.ShowAsync() == ContentDialogResult.Primary;
    }

    public static ProgressRing Busy() => new() { IsActive = true, Width = 40, Height = 40, Margin = new Thickness(0, 48, 0, 0), HorizontalAlignment = HorizontalAlignment.Center };

    public static StackPanel Page() => new() { Padding = new Thickness(28, 20, 28, 28), Spacing = 22 };
}
