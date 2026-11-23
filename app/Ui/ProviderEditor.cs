using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CloudStream;

/// <summary>Dialogs for creating providers: "add from link" detection and the full editor with a Test button.</summary>
public static class ProviderEditor
{
    private static readonly (string Kind, string Label)[] Kinds =
    {
        ("rss", "Feed (RSS or Atom)"),
        ("m3u", "Playlist (M3U)"),
        ("json", "JSON API"),
        ("html", "Web page (CSS selectors)"),
        ("peertube", "PeerTube instance"),
    };

    /// <summary>Asks for an address, detects what it is, then opens the editor prefilled. Returns true if something was saved.</summary>
    public static async Task<bool> AddFromLinkAsync(XamlRoot root)
    {
        var box = new TextBox { PlaceholderText = "https://..." };
        var note = Ui.Text("Paste a feed, playlist, API, PeerTube instance, YouTube channel or any page that lists videos. The app works out the type for you.", 12, false, 0.7);
        var panel = new StackPanel { Spacing = 10, MinWidth = 440 };
        panel.Children.Add(box);
        panel.Children.Add(note);
        var error = new TextBlock { TextWrapping = TextWrapping.Wrap, Visibility = Visibility.Collapsed };
        panel.Children.Add(error);

        var dlg = new ContentDialog
        {
            XamlRoot = root, Title = "Add provider from a link", Content = panel,
            PrimaryButtonText = "Detect", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Primary,
        };
        Detected? found = null;
        dlg.PrimaryButtonClick += async (_, args) =>
        {
            var deferral = args.GetDeferral();
            try
            {
                found = await Core.CallAsync<Detected>("provider.detect", new { url = box.Text.Trim() });
            }
            catch (Exception ex)
            {
                args.Cancel = true;
                error.Text = ex.Message;
                error.Visibility = Visibility.Visible;
            }
            finally { deferral.Complete(); }
        };
        if (await dlg.ShowAsync() != ContentDialogResult.Primary || found == null) return false;
        return await EditAsync(root, found.Definition, found.Note);
    }

    /// <summary>Opens the editor. Returns true if the provider was saved.</summary>
    public static async Task<bool> EditAsync(XamlRoot root, CustomDefinition? existing, string? note = null)
    {
        var def = existing ?? new CustomDefinition();
        TextBox Field(string value, string placeholder) => new() { Text = value, PlaceholderText = placeholder };

        var name = Field(def.Name, "My provider");
        var kind = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var (_, label) in Kinds) kind.Items.Add(label);
        kind.SelectedIndex = Math.Max(0, Array.FindIndex(Kinds, k => k.Kind == def.Kind));
        var url = Field(def.Url, "https://...");
        var search = Field(def.SearchUrl, "https://site/search?q={query}   (optional)");

        var list = Field(def.List, "");
        var title = Field(def.Title, "");
        var link = Field(def.Link, "");
        var image = Field(def.Image, "");
        var desc = Field(def.Description, "");
        var stream = Field(def.Stream, "video source@src   (optional)");
        var regex = Field(def.StreamRegex, "\"hls\":\"([^\"]+)\"   (optional)");

        var advanced = new StackPanel { Spacing = 8 };
        var listLabel = Ui.Text("", 12, false, 0.8);
        advanced.Children.Add(listLabel); advanced.Children.Add(list);
        advanced.Children.Add(Ui.Text("Title", 12, false, 0.8)); advanced.Children.Add(title);
        advanced.Children.Add(Ui.Text("Link", 12, false, 0.8)); advanced.Children.Add(link);
        advanced.Children.Add(Ui.Text("Image", 12, false, 0.8)); advanced.Children.Add(image);
        advanced.Children.Add(Ui.Text("Description", 12, false, 0.8)); advanced.Children.Add(desc);
        var htmlOnly = new StackPanel { Spacing = 8 };
        htmlOnly.Children.Add(Ui.Text("Stream selector (evaluated on the video's page)", 12, false, 0.8)); htmlOnly.Children.Add(stream);
        htmlOnly.Children.Add(Ui.Text("Stream pattern (regular expression on the page)", 12, false, 0.8)); htmlOnly.Children.Add(regex);
        advanced.Children.Add(htmlOnly);

        string CurrentKind() => Kinds[Math.Max(0, kind.SelectedIndex)].Kind;
        void Adapt()
        {
            var k = CurrentKind();
            advanced.Visibility = k is "json" or "html" ? Visibility.Visible : Visibility.Collapsed;
            htmlOnly.Visibility = k == "html" ? Visibility.Visible : Visibility.Collapsed;
            listLabel.Text = k == "json" ? "List path (dotted, for example data.videos)" : "Selector for one video card (for example article.card)";
            title.PlaceholderText = k == "json" ? "name   (optional)" : "h3   (optional)";
            link.PlaceholderText = k == "json" ? "url   (optional)" : "a@href   (optional)";
            image.PlaceholderText = k == "json" ? "thumbnail   (optional)" : "img@src   (optional)";
            desc.PlaceholderText = k == "json" ? "description   (optional)" : "p   (optional)";
        }
        kind.SelectionChanged += (_, _) => Adapt();
        Adapt();

        var result = new TextBlock { TextWrapping = TextWrapping.Wrap, Visibility = Visibility.Collapsed };
        var testBtn = new Button { Content = Ui.IconLabel(Symbol.Play, "Test") };

        CustomDefinition Build() => new()
        {
            Id = def.Id, Source = def.Source, Version = def.Version,
            Name = name.Text.Trim(), Kind = CurrentKind(), Url = url.Text.Trim(), SearchUrl = search.Text.Trim(),
            List = list.Text.Trim(), Title = title.Text.Trim(), Link = link.Text.Trim(), Image = image.Text.Trim(),
            Description = desc.Text.Trim(), Stream = stream.Text.Trim(), StreamRegex = regex.Text.Trim(),
        };

        testBtn.Click += async (_, _) =>
        {
            testBtn.IsEnabled = false;
            result.Visibility = Visibility.Visible;
            result.Text = "Testing...";
            try
            {
                var t = await Core.CallAsync<TestResult>("provider.test", new { definition = Build() });
                result.Text = t.Count == 0
                    ? "Loaded, but no videos were found. Check the address, selectors or field names."
                    : $"Found {t.Count} videos, for example: {string.Join("; ", t.Sample.Take(3))}";
            }
            catch (Exception ex) { result.Text = ex.Message; }
            finally { testBtn.IsEnabled = true; }
        };

        var form = new StackPanel { Spacing = 8, MinWidth = 480 };
        if (!string.IsNullOrEmpty(note)) form.Children.Add(Ui.Text(note, 12, false, 0.75));
        form.Children.Add(Ui.Text("Name", 12, false, 0.8)); form.Children.Add(name);
        form.Children.Add(Ui.Text("Type", 12, false, 0.8)); form.Children.Add(kind);
        form.Children.Add(Ui.Text("Address", 12, false, 0.8)); form.Children.Add(url);
        form.Children.Add(Ui.Text("Search address", 12, false, 0.8)); form.Children.Add(search);
        form.Children.Add(advanced);
        form.Children.Add(testBtn);
        form.Children.Add(result);
        form.Children.Add(Ui.Text("You are responsible for having the right to access what a provider lists. Protected (DRM) streams are not supported.", 11, false, 0.55));

        var dlg = new ContentDialog
        {
            XamlRoot = root, Title = existing == null ? "Create provider" : "Edit provider",
            Content = new ScrollViewer { Content = form, MaxHeight = 560 },
            PrimaryButtonText = "Save", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Primary,
        };
        dlg.PrimaryButtonClick += async (_, args) =>
        {
            var deferral = args.GetDeferral();
            try { await Core.CallAsync<JsonElement>("provider.save", new { definition = Build() }); }
            catch (Exception ex)
            {
                args.Cancel = true;
                result.Visibility = Visibility.Visible;
                result.Text = ex.Message;
            }
            finally { deferral.Complete(); }
        };
        return await dlg.ShowAsync() == ContentDialogResult.Primary;
    }
}
