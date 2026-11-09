using System;
using System.Collections.Generic;
using System.Text;
using System.Collections.Concurrent;
using System.Threading;
public sealed class DroneInput {
 readonly ConcurrentQueue<string> queue=new ConcurrentQueue<string>();
 public volatile bool Closed;
 public DroneInput(){var thread=new Thread(()=>{try{string line;while((line=Console.In.ReadLine())!=null)queue.Enqueue(line);}finally{Closed=true;}});thread.IsBackground=true;thread.Start();}
 public string Next(){string line;return queue.TryDequeue(out line)?line:null;}
}
public sealed class DroneFrame {public byte[] Bytes;public int System,Component,Message;public bool Verified,Signed;public Dictionary<string,object> Fields=new Dictionary<string,object>();}
public sealed class DroneMavlink {
 readonly List<byte> buffer=new List<byte>();
 static readonly Dictionary<int,byte> extra=new Dictionary<int,byte>{{0,50},{1,124},{21,159},{22,220},{24,24},{30,39},{33,104},{148,178},{253,83}};
 public static ushort Accumulate(ushort crc,byte value){byte tmp=(byte)(value^(byte)crc);tmp^=(byte)(tmp<<4);return (ushort)((crc>>8)^(tmp<<8)^(tmp<<3)^(tmp>>4));}
 public static byte[] ParameterRequest(byte system,byte component,byte sequence){byte[] frame={254,2,sequence,255,190,21,system,component,0,0};ushort crc=65535;for(int i=1;i<8;i++)crc=Accumulate(crc,frame[i]);crc=Accumulate(crc,159);frame[8]=(byte)crc;frame[9]=(byte)(crc>>8);return frame;}
 public List<DroneFrame> Push(byte[] bytes){
  if(buffer.Count+bytes.Length>131072)buffer.Clear();buffer.AddRange(bytes);int cursor=0;var frames=new List<DroneFrame>();
  while(cursor<buffer.Count){byte magic=buffer[cursor];if(magic!=254&&magic!=253){cursor++;continue;}int header=magic==253?10:6;if(buffer.Count-cursor<header)break;
   int length=buffer[cursor+1];bool signed=magic==253&&(buffer[cursor+2]&1)!=0;int total=header+length+2+(signed?13:0);if(buffer.Count-cursor<total)break;
   var frame=new DroneFrame{Bytes=buffer.GetRange(cursor,total).ToArray(),Signed=signed,System=buffer[cursor+(magic==253?5:3)],Component=buffer[cursor+(magic==253?6:4)]};
   frame.Message=magic==253?buffer[cursor+7]|buffer[cursor+8]<<8|buffer[cursor+9]<<16:buffer[cursor+5];
   byte crcExtra;if(extra.TryGetValue(frame.Message,out crcExtra)&&(magic!=253||(buffer[cursor+2]&254)==0)){
    ushort crc=65535;for(int i=1;i<header+length;i++)crc=Accumulate(crc,frame.Bytes[i]);crc=Accumulate(crc,crcExtra);
    frame.Verified=crc==(frame.Bytes[header+length]|frame.Bytes[header+length+1]<<8);
   }
   if(frame.Verified){byte[] payload=new byte[255];Array.Copy(frame.Bytes,header,payload,0,length);Decode(frame,payload);}
   frames.Add(frame);cursor+=total;
  }
  if(cursor>0)buffer.RemoveRange(0,cursor);return frames;
 }
 static void Decode(DroneFrame frame,byte[] p){var f=frame.Fields;
  if(frame.Message==0){f["type"]=(int)p[4];f["autopilot"]=(int)p[5];f["armed"]=(p[6]&128)!=0;f["system_status"]=(int)p[7];f["custom_mode"]=BitConverter.ToUInt32(p,0);}
  else if(frame.Message==1){f["battery_volts"]=BitConverter.ToUInt16(p,14)/1000.0;f["battery_percent"]=(sbyte)p[30];}
  else if(frame.Message==33){f["latitude"]=BitConverter.ToInt32(p,4)/1e7;f["longitude"]=BitConverter.ToInt32(p,8)/1e7;f["relative_altitude_m"]=BitConverter.ToInt32(p,16)/1000.0;}
  else if(frame.Message==30){f["roll_rad"]=BitConverter.ToSingle(p,4);f["pitch_rad"]=BitConverter.ToSingle(p,8);f["yaw_rad"]=BitConverter.ToSingle(p,12);}
  else if(frame.Message==22){f["parameter"]=Encoding.ASCII.GetString(p,8,16).TrimEnd('\0');f["wire_float"]=BitConverter.ToSingle(p,0);f["raw_value_hex"]=BitConverter.ToString(p,0,4);f["parameter_type"]=(int)p[24];f["parameter_count"]=BitConverter.ToUInt16(p,4);f["parameter_index"]=BitConverter.ToUInt16(p,6);}
  else if(frame.Message==148){f["flight_sw_version"]=BitConverter.ToUInt32(p,16);f["vendor_id"]=BitConverter.ToUInt16(p,32);f["product_id"]=BitConverter.ToUInt16(p,34);f["uid"]=BitConverter.ToUInt64(p,8).ToString();}
  else if(frame.Message==253){f["severity"]=(int)p[0];f["text"]=Encoding.UTF8.GetString(p,1,50).TrimEnd('\0');}
 }
}
