using StreamJsonRpc;
using System.Diagnostics;

namespace ethersync.src
{
    public class EtherSyncClientWrapper
    {
        private Process ethersyncClient;
        private JsonRpc rpc;
        private NotificationHandler notificationHandler;

        public EtherSyncClientWrapper()
        {

        }


        public void  StartClient()
        {

            ProcessStartInfo startInfo = new ProcessStartInfo
            {
                FileName = "ethersync",
                Arguments = "client",
                RedirectStandardOutput = true,
                RedirectStandardInput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            ethersyncClient = new Process
            {
                StartInfo = startInfo
            };

            ethersyncClient.Start();

            notificationHandler = new NotificationHandler();
            rpc = JsonRpc.Attach(ethersyncClient.StandardInput.BaseStream, ethersyncClient.StandardOutput.BaseStream, notificationHandler);
            //rpc.TraceSource.Switch.Level = SourceLevels.All; adds debug information to the console
            //rpc.TraceSource.Listeners.Add(new ConsoleTraceListener());
        }

        public void StopClient()
        {
            rpc.Dispose();
            rpc = null;
            ethersyncClient.Close();
            ethersyncClient = null;
        }
    }
}
