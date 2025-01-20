

using System;
using ethersync.src.model;

namespace ethersync.src
{
    public class NotificationHandler
    {
        public void edit(string uri, int revision, Delta[] delta)
        {
            Console.WriteLine($"edit empfangen!");
        }

        public void cursor(string name, Range[] ranges,string uri, string userid)
        {
            Console.WriteLine($"cursor empfangen!");
        }
    }
}
