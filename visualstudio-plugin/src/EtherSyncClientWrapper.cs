using System.Diagnostics;

namespace visualstudio_plugin.src
{
    internal class EtherSyncClientWrapper
    {
        private Process ethersyncClient;


        public void  StartClient()
        {

            ProcessStartInfo startInfo = new ProcessStartInfo
            {
                FileName = "ethersync",
                Arguments = "client",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true // set false to have console window of client
            };

            ethersyncClient = new Process
            {
                StartInfo = startInfo
            };

            ethersyncClient.Start();
        }

        public void StopClient()
        {
            ethersyncClient.Close();
        }
    }
}
