using Microsoft.UI.Xaml;

namespace CloudStream;

// CloudStream for Windows — an independent implementation inspired by
// https://github.com/recloudstream/cloudstream (GPL-3.0). Not endorsed by that project.
public partial class App : Application
{
    public static MainWindow? MainWin { get; private set; }
    public static string? StartupError { get; private set; }

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            e.Handled = true; // keep the app alive; the failing screen shows its own error state
            System.Diagnostics.Debug.WriteLine(e.Exception);
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try { Core.Init(AppState.DataDir); }
        catch (Exception ex) { StartupError = ex.Message; }

        MainWin = new MainWindow();
        MainWin.Activate();
    }
}
