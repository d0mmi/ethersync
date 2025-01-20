namespace ethersync.src.model
{
    public class Position
    {
        public int line { get; set; }
        public int character { get; set; }
    }

    public class Range
    {
        public Position start { get; set; }
        public Position end { get; set; }
    }

    public class Delta
    {
        public Range range { get; set; }
        public string replacement { get; set; }
    }

    public class Edit
    {
        public string uri { get; set; }
        public int revision { get; set; }
        public Delta[] delta { get; set; }
    }

    public class Cursor
    {
        public string uri { get; set; }
        public Range[] ranges { get; set; }
    }

    public class CursorFromDaemon
    {
        public string userid { get; set; }
        public string name { get; set; }
        public string uri { get; set; }
        public Range[] ranges { get; set; }
    }

    public class Revision
    {
        public int daemon { get; set; }
        public int editor { get; set; }
    }
}